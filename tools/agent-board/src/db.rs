//! SQLite storage: threads, summary revisions, posts, attachments and an FTS5 index.
//!
//! Nothing is deleted. A summary change inserts a new revision; closing a thread is a
//! status change; a refuted post is marked `superseded_by` a later one.

use std::collections::HashSet;
use std::path::Path;

use chrono::NaiveDate;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use crate::error::AppError;

/// Schema migrations, applied in order. `PRAGMA user_version` records how many ran.
///
/// The `search` table holds one row per thread (title + current summary) with rowid
/// `-thread_id`, so it can be replaced in place when the summary changes, and one row per
/// post and attachment with ordinary positive rowids.
const MIGRATIONS: &[&str] = &[
    r#"
CREATE TABLE threads (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('open', 'resolved', 'parked')),
    owner TEXT NOT NULL,
    created INTEGER NOT NULL,
    updated INTEGER NOT NULL,
    due TEXT,
    waiting_on TEXT,
    waiting_ref TEXT
);
CREATE TABLE thread_tags (
    thread INTEGER NOT NULL REFERENCES threads(id),
    tag TEXT NOT NULL,
    PRIMARY KEY (thread, tag)
) WITHOUT ROWID;
CREATE INDEX thread_tags_tag ON thread_tags(tag);
CREATE TABLE summaries (
    id INTEGER PRIMARY KEY,
    thread INTEGER NOT NULL REFERENCES threads(id),
    author TEXT NOT NULL,
    created INTEGER NOT NULL,
    body TEXT NOT NULL
);
CREATE INDEX summaries_thread ON summaries(thread, id);
CREATE TABLE posts (
    id INTEGER PRIMARY KEY,
    thread INTEGER NOT NULL REFERENCES threads(id),
    author TEXT NOT NULL,
    created INTEGER NOT NULL,
    body TEXT NOT NULL,
    reply_to INTEGER REFERENCES posts(id),
    ask TEXT,
    superseded_by INTEGER REFERENCES posts(id),
    links TEXT NOT NULL DEFAULT '[]'
);
CREATE INDEX posts_thread ON posts(thread, id);
CREATE INDEX posts_reply ON posts(reply_to);
CREATE INDEX posts_ask ON posts(ask) WHERE ask IS NOT NULL;
CREATE INDEX posts_author ON posts(author, thread);
CREATE TABLE attachments (
    id INTEGER PRIMARY KEY,
    post INTEGER NOT NULL REFERENCES posts(id),
    name TEXT NOT NULL,
    content TEXT NOT NULL
);
CREATE INDEX attachments_post ON attachments(post);
CREATE VIRTUAL TABLE search USING fts5(
    kind UNINDEXED, ref UNINDEXED, thread UNINDEXED, author UNINDEXED, created UNINDEXED,
    title, body, tokenize = 'porter unicode61'
);
"#,
    r#"
CREATE TABLE discord (
    id INTEGER PRIMARY KEY,
    channel INTEGER NOT NULL,
    thread INTEGER,
    author TEXT NOT NULL,
    author_kind TEXT NOT NULL,
    created INTEGER NOT NULL,
    edited INTEGER,
    reply_to INTEGER,
    content TEXT NOT NULL,
    attachments TEXT NOT NULL DEFAULT '[]',
    url TEXT NOT NULL,
    search_row INTEGER NOT NULL
);
CREATE INDEX discord_place ON discord(channel, thread, id);
CREATE INDEX discord_created ON discord(created);
"#,
    r#"
CREATE TABLE status (
    agent TEXT NOT NULL,
    key TEXT NOT NULL,
    updated INTEGER NOT NULL,
    ttl INTEGER NOT NULL,
    body TEXT NOT NULL,
    PRIMARY KEY (agent, key)
) WITHOUT ROWID;
"#,
    r#"
CREATE TABLE web_logins (
    hash BLOB PRIMARY KEY,
    who TEXT NOT NULL,
    discord_id TEXT NOT NULL,
    created INTEGER NOT NULL,
    expires INTEGER NOT NULL
) WITHOUT ROWID;
CREATE TABLE web_sessions (
    id INTEGER PRIMARY KEY,
    hash BLOB NOT NULL UNIQUE,
    who TEXT NOT NULL,
    discord_id TEXT NOT NULL,
    created INTEGER NOT NULL,
    expires INTEGER NOT NULL,
    last_used INTEGER NOT NULL
);
"#,
];

pub const MAX_TITLE: usize = 200;
pub const MAX_BODY: usize = 256 * 1024;
pub const MAX_ATTACHMENT: usize = 1024 * 1024;
pub const MAX_ATTACHMENTS: usize = 10;
pub const MAX_TAGS: usize = 10;

/// Thread status. Threads are never deleted, only resolved or parked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Open,
    Resolved,
    Parked,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Open => "open",
            Status::Resolved => "resolved",
            Status::Parked => "parked",
        }
    }

    fn parse(text: &str) -> rusqlite::Result<Self> {
        match text {
            "open" => Ok(Status::Open),
            "resolved" => Ok(Status::Resolved),
            "parked" => Ok(Status::Parked),
            other => Err(rusqlite::Error::InvalidColumnType(
                0,
                format!("status {other}"),
                rusqlite::types::Type::Text,
            )),
        }
    }
}

/// A thread with its current summary.
#[derive(Clone, Debug, Serialize)]
pub struct Thread {
    pub id: i64,
    pub title: String,
    pub status: Status,
    pub owner: String,
    pub tags: Vec<String>,
    pub created: i64,
    pub updated: i64,
    pub due: Option<String>,
    pub waiting_on: Option<String>,
    pub waiting_ref: Option<String>,
    /// The full current summary in `read`; only its first line in listings.
    pub summary: String,
    pub summary_author: Option<String>,
    pub summary_updated: Option<i64>,
    pub summary_revisions: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct AttachmentInfo {
    pub id: i64,
    pub name: String,
    pub size: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Post {
    pub id: i64,
    pub thread: i64,
    pub author: String,
    pub created: i64,
    pub body: String,
    pub reply_to: Option<i64>,
    pub ask: Option<String>,
    /// For a post with `ask`: whether the asked party has posted a reply to it.
    pub answered: Option<bool>,
    pub superseded_by: Option<i64>,
    pub links: Vec<String>,
    pub attachments: Vec<AttachmentInfo>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ThreadView {
    pub thread: Thread,
    pub posts: Vec<Post>,
    /// Posts in the thread older than the ones returned.
    pub earlier_posts: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct SummaryRevision {
    pub id: i64,
    pub author: String,
    pub created: i64,
    pub body: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Attachment {
    pub id: i64,
    pub post: i64,
    pub thread: i64,
    pub name: String,
    pub content: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct SearchHit {
    pub kind: String,
    /// Thread id for `thread` hits, post id for `post`, attachment id for `attachment`,
    /// message id for `discord`.
    pub id: i64,
    /// The board thread; none for `discord` hits.
    pub thread: Option<i64>,
    pub thread_title: Option<String>,
    pub author: String,
    pub created: i64,
    pub snippet: String,
    pub superseded: bool,
}

/// One unanswered question, for the session-start briefing and the dashboard.
#[derive(Clone, Debug, Serialize)]
pub struct Ask {
    pub post: i64,
    pub thread: i64,
    pub thread_title: String,
    pub author: String,
    /// Who is asked.
    pub asked: String,
    pub created: i64,
    pub first_line: String,
}

/// What an agent should look at when a session starts, most urgent first.
#[derive(Clone, Debug, Serialize)]
pub struct Briefing {
    pub agent: String,
    pub since: Option<i64>,
    pub asks: Vec<Ask>,
    pub waiting_on_you: Vec<Thread>,
    pub waiting_on_others: Vec<Thread>,
    pub due: Vec<Thread>,
    pub changed: Vec<Thread>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ThreadFilter {
    /// `open` (default), `resolved`, `parked` or `all`.
    pub status: Option<String>,
    pub tag: Option<String>,
    pub owner: Option<String>,
    pub waiting_on: Option<String>,
    /// Only threads this agent owns or posted in.
    pub involved: Option<String>,
    pub updated_since: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct NewAttachment {
    pub name: String,
    pub content: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct NewPost {
    pub body: String,
    pub reply_to: Option<i64>,
    pub ask: Option<String>,
    #[serde(default)]
    pub links: Vec<String>,
    #[serde(default)]
    pub attachments: Vec<NewAttachment>,
    #[serde(default)]
    pub supersedes: Vec<i64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct NewThread {
    pub title: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub summary: String,
    pub due: Option<String>,
    pub waiting_on: Option<String>,
    pub waiting_ref: Option<String>,
    /// An optional first post.
    pub post: Option<NewPost>,
}

/// A change to a thread's state. Absent fields are left alone; an empty string clears
/// `due` and `waiting_on` (which also clears `waiting_ref`).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ThreadUpdate {
    pub summary: Option<String>,
    pub status: Option<Status>,
    pub title: Option<String>,
    pub owner: Option<String>,
    pub tags: Option<Vec<String>>,
    pub due: Option<String>,
    pub waiting_on: Option<String>,
    pub waiting_ref: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct SearchQuery {
    pub q: String,
    /// `thread`, `post` or `attachment`; all kinds when absent.
    pub kind: Option<String>,
    pub author: Option<String>,
    pub since: Option<i64>,
    pub until: Option<i64>,
    pub limit: Option<i64>,
}

pub struct Board {
    pub(crate) conn: Connection,
}

pub(crate) fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn first_line(text: &str) -> String {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim()
        .to_owned()
}

fn bad(message: impl Into<String>) -> AppError {
    AppError::BadRequest(message.into())
}

fn check_text(what: &str, text: &str, max: usize) -> Result<(), AppError> {
    if text.trim().is_empty() {
        return Err(bad(format!("{what} is empty")));
    }
    if text.len() > max {
        return Err(bad(format!("{what} is longer than {max} bytes")));
    }
    Ok(())
}

fn check_tags(tags: &[String]) -> Result<(), AppError> {
    if tags.len() > MAX_TAGS {
        return Err(bad(format!("at most {MAX_TAGS} tags")));
    }
    for tag in tags {
        let valid = !tag.is_empty()
            && tag.len() <= 32
            && tag
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !valid {
            return Err(bad(format!("tag {tag:?}: use 1-32 of a-z, 0-9 and -")));
        }
    }
    Ok(())
}

fn check_due(due: &str) -> Result<(), AppError> {
    if due.is_empty() || NaiveDate::parse_from_str(due, "%Y-%m-%d").is_ok() {
        Ok(())
    } else {
        Err(bad(format!("due {due:?}: use YYYY-MM-DD")))
    }
}

/// Maps "" to NULL, for the fields where an empty string means "clear".
fn non_empty(value: &str) -> Option<&str> {
    (!value.is_empty()).then_some(value)
}

const THREAD_COLUMNS: &str = "t.id, t.title, t.status, t.owner, t.created, t.updated, t.due, \
     t.waiting_on, t.waiting_ref, \
     (SELECT group_concat(tag, ' ') FROM (SELECT tag FROM thread_tags WHERE thread = t.id \
        ORDER BY tag)), \
     s.body, s.author, s.created, \
     (SELECT count(*) FROM summaries WHERE thread = t.id)";

/// Joins each thread with its latest summary revision.
const THREAD_FROM: &str = "threads t LEFT JOIN summaries s ON s.id = \
     (SELECT max(id) FROM summaries WHERE thread = t.id)";

fn thread_from_row(row: &Row, full_summary: bool) -> rusqlite::Result<Thread> {
    let tags: Option<String> = row.get(9)?;
    let summary: Option<String> = row.get(10)?;
    let summary = summary.unwrap_or_default();
    Ok(Thread {
        id: row.get(0)?,
        title: row.get(1)?,
        status: Status::parse(&row.get::<_, String>(2)?)?,
        owner: row.get(3)?,
        created: row.get(4)?,
        updated: row.get(5)?,
        due: row.get(6)?,
        waiting_on: row.get(7)?,
        waiting_ref: row.get(8)?,
        tags: tags
            .map(|t| t.split(' ').map(str::to_owned).collect())
            .unwrap_or_default(),
        summary: if full_summary {
            summary
        } else {
            first_line(&summary)
        },
        summary_author: row.get(11)?,
        summary_updated: row.get(12)?,
        summary_revisions: row.get(13)?,
    })
}

/// SQL condition: `?agent` owns thread `t` or has posted in it.
const INVOLVED: &str =
    "(t.owner = :agent OR EXISTS (SELECT 1 FROM posts ip WHERE ip.thread = t.id AND ip.author = :agent))";

impl Board {
    /// Opens (creating if needed) the board database and applies pending migrations.
    ///
    /// # Errors
    ///
    /// Fails if the file can't be opened or a migration fails.
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        Self::init(Connection::open(path)?)
    }

    /// An in-memory board, for tests.
    #[cfg(test)]
    pub fn open_in_memory() -> anyhow::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> anyhow::Result<Self> {
        // WAL with synchronous=FULL: a commit is on disk before the API answers.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "FULL")?;
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        for (index, migration) in (0_i64..).zip(MIGRATIONS).skip(version.try_into()?) {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(migration)?;
            tx.pragma_update(None, "user_version", index + 1)?;
            tx.commit()?;
        }
        Ok(Self { conn })
    }

    /// Writes a consistent copy of the database to `target` (which must not exist).
    ///
    /// # Errors
    ///
    /// Fails if SQLite can't write the copy.
    pub fn backup_to(&self, target: &Path) -> anyhow::Result<()> {
        let target = target
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("non-UTF-8 backup path"))?;
        self.conn.execute("VACUUM INTO ?1", [target])?;
        Ok(())
    }

    /// Lists threads matching `filter`, most recently updated first.
    ///
    /// # Errors
    ///
    /// Returns `BadRequest` for an unknown status.
    pub fn list_threads(&self, filter: &ThreadFilter) -> Result<Vec<Thread>, AppError> {
        let status = filter.status.as_deref().unwrap_or("open");
        if !matches!(status, "open" | "resolved" | "parked" | "all") {
            return Err(bad(format!(
                "status {status:?}: use open, resolved, parked or all"
            )));
        }
        let sql = format!(
            "SELECT {THREAD_COLUMNS} FROM {THREAD_FROM} WHERE \
             (:status = 'all' OR t.status = :status) \
             AND (:tag IS NULL OR EXISTS (SELECT 1 FROM thread_tags WHERE thread = t.id \
                  AND tag = :tag)) \
             AND (:owner IS NULL OR t.owner = :owner) \
             AND (:waiting IS NULL OR t.waiting_on = :waiting) \
             AND (:since IS NULL OR t.updated >= :since) \
             AND (:agent IS NULL OR {INVOLVED}) \
             ORDER BY t.updated DESC, t.id DESC"
        );
        let mut statement = self.conn.prepare_cached(&sql)?;
        let rows = statement.query_map(
            rusqlite::named_params! {
                ":status": status,
                ":tag": filter.tag,
                ":owner": filter.owner,
                ":waiting": filter.waiting_on,
                ":since": filter.updated_since,
                ":agent": filter.involved,
            },
            |row| thread_from_row(row, false),
        )?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn thread(&self, id: i64) -> Result<Thread, AppError> {
        let sql = format!("SELECT {THREAD_COLUMNS} FROM {THREAD_FROM} WHERE t.id = ?1");
        self.conn
            .query_row(&sql, [id], |row| thread_from_row(row, true))
            .optional()?
            .ok_or_else(|| AppError::NotFound(format!("thread {id}")))
    }

    fn posts_where(&self, condition: &str, args: &[i64]) -> Result<Vec<Post>, AppError> {
        let sql = format!(
            "SELECT p.id, p.thread, p.author, p.created, p.body, p.reply_to, p.ask, \
                    p.superseded_by, p.links, \
                    CASE WHEN p.ask IS NULL THEN NULL ELSE EXISTS (SELECT 1 FROM posts r \
                        WHERE r.reply_to = p.id AND r.author = p.ask) END \
             FROM posts p WHERE {condition}"
        );
        let mut statement = self.conn.prepare_cached(&sql)?;
        let mut posts = statement
            .query_map(params_from_iter(args), |row| {
                let links: String = row.get(8)?;
                Ok(Post {
                    id: row.get(0)?,
                    thread: row.get(1)?,
                    author: row.get(2)?,
                    created: row.get(3)?,
                    body: row.get(4)?,
                    reply_to: row.get(5)?,
                    ask: row.get(6)?,
                    superseded_by: row.get(7)?,
                    links: serde_json::from_str(&links).unwrap_or_default(),
                    answered: row.get(9)?,
                    attachments: Vec::new(),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut attachments = self.conn.prepare_cached(
            "SELECT id, name, length(CAST(content AS BLOB)) FROM attachments WHERE post = ?1 \
             ORDER BY id",
        )?;
        for post in &mut posts {
            post.attachments = attachments
                .query_map([post.id], |row| {
                    Ok(AttachmentInfo {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        size: row.get(2)?,
                    })
                })?
                .collect::<rusqlite::Result<_>>()?;
        }
        Ok(posts)
    }

    /// A thread with its full summary and posts, oldest first.
    ///
    /// `since_post` returns only posts after that id; `limit` keeps only the newest ones.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` for an unknown thread.
    pub fn read_thread(
        &self,
        id: i64,
        since_post: Option<i64>,
        limit: Option<i64>,
    ) -> Result<ThreadView, AppError> {
        let thread = self.thread(id)?;
        let since = since_post.unwrap_or(0);
        let limit = limit.unwrap_or(i64::MAX).max(1);
        let mut posts = self.posts_where(
            "p.thread = ?1 AND p.id > ?2 ORDER BY p.id DESC LIMIT ?3",
            &[id, since, limit],
        )?;
        posts.reverse();
        let earlier_posts = match posts.first() {
            Some(first) => self.conn.query_row(
                "SELECT count(*) FROM posts WHERE thread = ?1 AND id < ?2",
                [id, first.id],
                |row| row.get(0),
            )?,
            None => 0,
        };
        Ok(ThreadView {
            thread,
            posts,
            earlier_posts,
        })
    }

    /// All revisions of a thread's summary, newest first.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` for an unknown thread.
    pub fn summary_history(&self, thread: i64) -> Result<Vec<SummaryRevision>, AppError> {
        self.thread(thread)?;
        let mut statement = self.conn.prepare_cached(
            "SELECT id, author, created, body FROM summaries WHERE thread = ?1 ORDER BY id DESC",
        )?;
        let rows = statement.query_map([thread], |row| {
            Ok(SummaryRevision {
                id: row.get(0)?,
                author: row.get(1)?,
                created: row.get(2)?,
                body: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// One attachment with its content.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` for an unknown attachment.
    pub fn attachment(&self, id: i64) -> Result<Attachment, AppError> {
        self.conn
            .query_row(
                "SELECT a.id, a.post, p.thread, a.name, a.content FROM attachments a \
                 JOIN posts p ON p.id = a.post WHERE a.id = ?1",
                [id],
                |row| {
                    Ok(Attachment {
                        id: row.get(0)?,
                        post: row.get(1)?,
                        thread: row.get(2)?,
                        name: row.get(3)?,
                        content: row.get(4)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| AppError::NotFound(format!("attachment {id}")))
    }

    /// Replaces the thread's row in the search index with its current title and summary.
    fn index_thread(&self, id: i64) -> Result<(), AppError> {
        let thread = self.thread(id)?;
        self.conn
            .execute("DELETE FROM search WHERE rowid = ?1", [-id])?;
        self.conn.execute(
            "INSERT INTO search (rowid, kind, ref, thread, author, created, title, body) \
             VALUES (?1, 'thread', ?2, ?2, ?3, ?4, ?5, ?6)",
            params![
                -id,
                id,
                thread.owner,
                thread.updated,
                thread.title,
                thread.summary
            ],
        )?;
        Ok(())
    }

    fn set_tags(&self, thread: i64, tags: &[String]) -> Result<(), AppError> {
        self.conn
            .execute("DELETE FROM thread_tags WHERE thread = ?1", [thread])?;
        let unique: HashSet<&String> = tags.iter().collect();
        for tag in unique {
            self.conn.execute(
                "INSERT INTO thread_tags (thread, tag) VALUES (?1, ?2)",
                params![thread, tag],
            )?;
        }
        Ok(())
    }

    fn insert_post(
        &self,
        author: &str,
        thread: i64,
        post: &NewPost,
        at: i64,
    ) -> Result<i64, AppError> {
        check_text("body", &post.body, MAX_BODY)?;
        if post.attachments.len() > MAX_ATTACHMENTS {
            return Err(bad(format!("at most {MAX_ATTACHMENTS} attachments")));
        }
        for attachment in &post.attachments {
            check_text("attachment name", &attachment.name, 200)?;
            if attachment.content.len() > MAX_ATTACHMENT {
                return Err(bad(format!(
                    "attachment {:?} is longer than {MAX_ATTACHMENT} bytes",
                    attachment.name
                )));
            }
        }
        if let Some(ask) = &post.ask {
            check_text("ask", ask, 64)?;
        }
        let exists = |id: i64| -> Result<bool, AppError> {
            Ok(self.conn.query_row(
                "SELECT EXISTS (SELECT 1 FROM posts WHERE id = ?1)",
                [id],
                |row| row.get(0),
            )?)
        };
        for id in post.reply_to.iter().chain(&post.supersedes) {
            if !exists(*id)? {
                return Err(AppError::NotFound(format!("post {id}")));
            }
        }
        self.conn.execute(
            "INSERT INTO posts (thread, author, created, body, reply_to, ask, links) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                thread,
                author,
                at,
                post.body,
                post.reply_to,
                post.ask.as_deref().and_then(non_empty),
                serde_json::to_string(&post.links).map_err(anyhow::Error::from)?,
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        self.conn.execute(
            "INSERT INTO search (kind, ref, thread, author, created, title, body) \
             VALUES ('post', ?1, ?2, ?3, ?4, '', ?5)",
            params![id, thread, author, at, post.body],
        )?;
        for attachment in &post.attachments {
            self.conn.execute(
                "INSERT INTO attachments (post, name, content) VALUES (?1, ?2, ?3)",
                params![id, attachment.name, attachment.content],
            )?;
            self.conn.execute(
                "INSERT INTO search (kind, ref, thread, author, created, title, body) \
                 VALUES ('attachment', ?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    self.conn.last_insert_rowid(),
                    thread,
                    author,
                    at,
                    attachment.name,
                    attachment.content
                ],
            )?;
        }
        for old in &post.supersedes {
            self.conn.execute(
                "UPDATE posts SET superseded_by = ?1 WHERE id = ?2",
                params![id, old],
            )?;
        }
        self.conn.execute(
            "UPDATE threads SET updated = ?1 WHERE id = ?2",
            params![at, thread],
        )?;
        Ok(id)
    }

    /// Creates a thread owned by `author`, optionally with a first post.
    ///
    /// Returns the thread id and the first post's id.
    ///
    /// # Errors
    ///
    /// Returns `BadRequest` for invalid fields.
    pub fn create_thread(
        &self,
        author: &str,
        new: &NewThread,
    ) -> Result<(i64, Option<i64>), AppError> {
        check_text("title", &new.title, MAX_TITLE)?;
        check_tags(&new.tags)?;
        if new.summary.len() > MAX_BODY {
            return Err(bad(format!("summary is longer than {MAX_BODY} bytes")));
        }
        if let Some(due) = &new.due {
            check_due(due)?;
        }
        let at = now();
        // The mutex around the board makes this connection ours alone.
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO threads (title, status, owner, created, updated, due, waiting_on, \
             waiting_ref) VALUES (?1, 'open', ?2, ?3, ?3, ?4, ?5, ?6)",
            params![
                new.title.trim(),
                author,
                at,
                new.due.as_deref().and_then(non_empty),
                new.waiting_on.as_deref().and_then(non_empty),
                new.waiting_ref.as_deref().and_then(non_empty),
            ],
        )?;
        let id = tx.last_insert_rowid();
        self.set_tags(id, &new.tags)?;
        if !new.summary.trim().is_empty() {
            tx.execute(
                "INSERT INTO summaries (thread, author, created, body) VALUES (?1, ?2, ?3, ?4)",
                params![id, author, at, new.summary],
            )?;
        }
        let post = match &new.post {
            Some(post) => Some(self.insert_post(author, id, post, at)?),
            None => None,
        };
        self.index_thread(id)?;
        tx.commit()?;
        Ok((id, post))
    }

    /// Adds a post to a thread.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` for an unknown thread or referenced post.
    pub fn add_post(&self, author: &str, thread: i64, post: &NewPost) -> Result<i64, AppError> {
        // The mutex around the board makes this connection ours alone.
        let tx = self.conn.unchecked_transaction()?;
        self.thread(thread)?;
        let id = self.insert_post(author, thread, post, now())?;
        self.index_thread(thread)?;
        tx.commit()?;
        Ok(id)
    }

    /// Applies `update` to a thread; a changed summary becomes a new revision.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` for an unknown thread, `BadRequest` for invalid fields.
    pub fn update_thread(
        &self,
        author: &str,
        id: i64,
        update: &ThreadUpdate,
    ) -> Result<Thread, AppError> {
        // The mutex around the board makes this connection ours alone.
        let tx = self.conn.unchecked_transaction()?;
        let current = self.thread(id)?;
        let at = now();
        if let Some(summary) = &update.summary {
            if summary.len() > MAX_BODY {
                return Err(bad(format!("summary is longer than {MAX_BODY} bytes")));
            }
            if *summary != current.summary {
                tx.execute(
                    "INSERT INTO summaries (thread, author, created, body) VALUES (?1, ?2, ?3, ?4)",
                    params![id, author, at, summary],
                )?;
            }
        }
        if let Some(title) = &update.title {
            check_text("title", title, MAX_TITLE)?;
            tx.execute(
                "UPDATE threads SET title = ?1 WHERE id = ?2",
                params![title.trim(), id],
            )?;
        }
        if let Some(owner) = &update.owner {
            check_text("owner", owner, 64)?;
            tx.execute(
                "UPDATE threads SET owner = ?1 WHERE id = ?2",
                params![owner, id],
            )?;
        }
        if let Some(status) = update.status {
            tx.execute(
                "UPDATE threads SET status = ?1 WHERE id = ?2",
                params![status.as_str(), id],
            )?;
        }
        if let Some(tags) = &update.tags {
            check_tags(tags)?;
            self.set_tags(id, tags)?;
        }
        if let Some(due) = &update.due {
            check_due(due)?;
            tx.execute(
                "UPDATE threads SET due = ?1 WHERE id = ?2",
                params![non_empty(due), id],
            )?;
        }
        if let Some(waiting_on) = &update.waiting_on {
            tx.execute(
                "UPDATE threads SET waiting_on = ?1, waiting_ref = ?2 WHERE id = ?3",
                params![
                    non_empty(waiting_on),
                    non_empty(waiting_on).and(update.waiting_ref.as_deref().and_then(non_empty)),
                    id
                ],
            )?;
        } else if let Some(waiting_ref) = &update.waiting_ref {
            tx.execute(
                "UPDATE threads SET waiting_ref = ?1 WHERE id = ?2",
                params![non_empty(waiting_ref), id],
            )?;
        }
        tx.execute(
            "UPDATE threads SET updated = ?1 WHERE id = ?2",
            params![at, id],
        )?;
        self.index_thread(id)?;
        let thread = self.thread(id)?;
        tx.commit()?;
        Ok(thread)
    }

    /// Full-text search over threads (title + summary), posts and attachments.
    ///
    /// The query is FTS5 syntax; if it doesn't parse, each word is searched literally.
    /// Superseded posts rank lower.
    ///
    /// # Errors
    ///
    /// Returns `BadRequest` for an empty query or unknown kind.
    pub fn search(&self, query: &SearchQuery) -> Result<Vec<SearchHit>, AppError> {
        if query.q.trim().is_empty() {
            return Err(bad("empty query"));
        }
        if let Some(kind) = &query.kind {
            if !matches!(kind.as_str(), "thread" | "post" | "attachment" | "discord") {
                return Err(bad(format!(
                    "kind {kind:?}: use thread, post, attachment or discord"
                )));
            }
        }
        match self.search_raw(&query.q, query) {
            Err(AppError::Sqlite(rusqlite::Error::SqliteFailure(_, Some(message))))
                if message.contains("fts5") || message.contains("syntax") =>
            {
                let literal = query
                    .q
                    .split_whitespace()
                    .map(|word| format!("\"{}\"", word.replace('"', "\"\"")))
                    .collect::<Vec<_>>()
                    .join(" ");
                self.search_raw(&literal, query)
            }
            other => other,
        }
    }

    fn search_raw(&self, fts_query: &str, query: &SearchQuery) -> Result<Vec<SearchHit>, AppError> {
        let mut statement = self.conn.prepare_cached(
            "SELECT s.kind, s.ref, s.thread, t.title, s.author, s.created, \
                    snippet(search, -1, '[', ']', ' ... ', 24), \
                    p.superseded_by IS NOT NULL \
             FROM search s LEFT JOIN threads t ON t.id = s.thread \
             LEFT JOIN posts p ON s.kind = 'post' AND p.id = s.ref \
             WHERE search MATCH :q \
               AND (:kind IS NULL OR s.kind = :kind) \
               AND (:author IS NULL OR s.author = :author) \
               AND (:since IS NULL OR s.created >= :since) \
               AND (:until IS NULL OR s.created < :until) \
             ORDER BY bm25(search, 0, 0, 0, 0, 0, 3.0, 1.0) \
                      + (CASE WHEN p.superseded_by IS NULL THEN 0 ELSE 5 END) \
             LIMIT :limit",
        )?;
        let rows = statement.query_map(
            rusqlite::named_params! {
                ":q": fts_query,
                ":kind": query.kind,
                ":author": query.author,
                ":since": query.since,
                ":until": query.until,
                ":limit": query.limit.unwrap_or(20).clamp(1, 200),
            },
            |row| {
                Ok(SearchHit {
                    kind: row.get(0)?,
                    id: row.get(1)?,
                    thread: row.get(2)?,
                    thread_title: row.get(3)?,
                    author: row.get(4)?,
                    created: row.get(5)?,
                    snippet: row.get(6)?,
                    superseded: row.get(7)?,
                })
            },
        )?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The session-start briefing for `agent`.
    ///
    /// `changed` lists open threads the agent is involved in whose summary someone else
    /// changed after `since`, which defaults to the agent's last post or summary edit.
    ///
    /// # Errors
    ///
    /// Only on database errors.
    pub fn briefing(&self, agent: &str, since: Option<i64>) -> Result<Briefing, AppError> {
        let since = match since {
            Some(since) => Some(since),
            None => self.conn.query_row(
                "SELECT max(at) FROM (SELECT max(created) AS at FROM posts WHERE author = ?1 \
                 UNION ALL SELECT max(created) FROM summaries WHERE author = ?1)",
                [agent],
                |row| row.get(0),
            )?,
        };
        let asks = self.open_asks(Some(agent))?;
        let threads = |condition: &str, order: &str| -> Result<Vec<Thread>, AppError> {
            let sql = format!(
                "SELECT {THREAD_COLUMNS} FROM {THREAD_FROM} \
                 WHERE t.status = 'open' AND {condition} ORDER BY {order}"
            );
            let mut statement = self.conn.prepare(&sql)?;
            // rusqlite rejects named parameters the statement doesn't use.
            let mut bound: Vec<(&str, &dyn rusqlite::ToSql)> = Vec::new();
            for (name, value) in [
                (":agent", &agent as &dyn rusqlite::ToSql),
                (":since", &since),
            ] {
                if statement.parameter_index(name)?.is_some() {
                    bound.push((name, value));
                }
            }
            let rows = statement.query_map(bound.as_slice(), |row| thread_from_row(row, false))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        };
        Ok(Briefing {
            agent: agent.to_owned(),
            since,
            asks,
            waiting_on_you: threads("t.waiting_on = :agent", "t.updated DESC")?,
            waiting_on_others: threads(
                &format!("t.waiting_on IS NOT NULL AND t.waiting_on != :agent AND {INVOLVED}"),
                "t.updated DESC",
            )?,
            due: threads("t.due IS NOT NULL", "t.due, t.id")?,
            changed: threads(
                &format!(
                    "{INVOLVED} AND s.author != :agent \
                     AND (:since IS NULL OR s.created > :since)"
                ),
                "s.created DESC",
            )?,
        })
    }

    /// Unanswered questions in open threads: those asked of `agent`, or all of them.
    pub(crate) fn open_asks(&self, agent: Option<&str>) -> Result<Vec<Ask>, AppError> {
        let mut statement = self.conn.prepare_cached(
            "SELECT p.id, p.thread, t.title, p.author, p.ask, p.created, p.body \
             FROM posts p JOIN threads t ON t.id = p.thread \
             WHERE p.ask IS NOT NULL AND (?1 IS NULL OR p.ask = ?1) \
               AND t.status = 'open' AND p.superseded_by IS NULL \
               AND NOT EXISTS (SELECT 1 FROM posts r WHERE r.reply_to = p.id AND r.author = p.ask) \
             ORDER BY p.id",
        )?;
        let asks = statement
            .query_map([agent], |row| {
                let body: String = row.get(6)?;
                Ok(Ask {
                    post: row.get(0)?,
                    thread: row.get(1)?,
                    thread_title: row.get(2)?,
                    author: row.get(3)?,
                    asked: row.get(4)?,
                    created: row.get(5)?,
                    first_line: first_line(&body),
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(asks)
    }

    /// Open threads that wait on someone, and open threads with a due date (soonest first).
    pub(crate) fn waiting_and_due(&self) -> Result<(Vec<Thread>, Vec<Thread>), AppError> {
        let threads = |condition: &str, order: &str| -> Result<Vec<Thread>, AppError> {
            let sql = format!(
                "SELECT {THREAD_COLUMNS} FROM {THREAD_FROM} \
                 WHERE t.status = 'open' AND {condition} ORDER BY {order}"
            );
            let mut statement = self.conn.prepare(&sql)?;
            let rows = statement.query_map([], |row| thread_from_row(row, false))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        };
        Ok((
            threads("t.waiting_on IS NOT NULL", "t.updated DESC")?,
            threads("t.due IS NOT NULL", "t.due, t.id")?,
        ))
    }
}
