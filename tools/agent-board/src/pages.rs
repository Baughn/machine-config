//! Wiki pages: what is true now, one page per topic, under a slug like `mod/ea`.
//!
//! Every write is a new revision (author, note, sources), made against the revision the
//! writer read (`base_rev`), so two agents can't silently overwrite each other. Nothing is
//! deleted. Each page has one row in the search index, replaced in place.

use std::collections::BTreeSet;

use rusqlite::{named_params, params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use crate::db::{bad, check_tags, check_text, now, visible, Board, MAX_PAGE_BODY};
use crate::diff::{diff_lines, hunks, Hunk};
use crate::error::AppError;

const MAX_SLUG: usize = 80;
const MAX_SEGMENTS: usize = 4;
const MAX_SEGMENT: usize = 40;
/// Titles, descriptions, notes and sources are counted in characters, not bytes.
const MAX_PAGE_TITLE: usize = 200;
const MAX_DESCRIPTION: usize = 300;
const MAX_NOTE: usize = 500;
const MAX_SOURCES: usize = 50;
const MAX_SOURCE: usize = 200;
/// Single-segment slugs the web view uses for itself (`/w/log`).
const RESERVED_SLUGS: [&str; 1] = ["log"];
/// Unchanged lines shown around each change in a diff.
const DIFF_CONTEXT: usize = 3;
/// Revisions in the page log, by default and at most.
const LOG_DEFAULT: i64 = 100;
const LOG_MAX: i64 = 500;

/// A page as listed, at one revision (the current one unless an old one was asked for).
#[derive(Clone, Debug, Serialize)]
pub struct PageInfo {
    pub id: i64,
    pub slug: String,
    /// Title and description as of `rev`.
    pub title: String,
    pub description: String,
    pub owner: String,
    pub tags: Vec<String>,
    pub rev: i64,
    pub created: i64,
    /// When `rev` was written, and by whom.
    pub updated: i64,
    pub updated_by: String,
    /// The body's size in bytes.
    pub size: i64,
    /// Set for a page in a private agent's sector: only that agent sees it over the API.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_to: Option<String>,
}

impl PageInfo {
    /// Whether `viewer` (an agent id) may see this page over the API.
    pub fn visible_to(&self, viewer: &str) -> bool {
        visible(self.private_to.as_deref(), viewer)
    }
}

/// A page with the body, note and sources of one revision.
#[derive(Clone, Debug, Serialize)]
pub struct Page {
    // `flatten` puts PageInfo's fields at the top level of the JSON object.
    #[serde(flatten)]
    pub info: PageInfo,
    pub body: String,
    pub note: String,
    pub sources: Vec<String>,
    /// The page's newest revision; equal to `info.rev` unless an old one was asked for.
    pub current_rev: i64,
}

/// One revision in a page's history, without its body.
#[derive(Clone, Debug, Serialize)]
pub struct Revision {
    pub rev: i64,
    pub author: String,
    pub created: i64,
    pub note: String,
    pub sources: Vec<String>,
    pub size: i64,
    pub title: String,
}

/// One revision in the log of all pages, newest first.
#[derive(Clone, Debug, Serialize)]
pub struct LogEntry {
    pub slug: String,
    /// The title as of this revision.
    pub title: String,
    pub rev: i64,
    pub author: String,
    pub created: i64,
    pub note: String,
    pub size: i64,
    /// The page's private owner, for filtering; never shown.
    #[serde(skip)]
    pub private_to: Option<String>,
}

impl LogEntry {
    pub fn visible_to(&self, viewer: &str) -> bool {
        visible(self.private_to.as_deref(), viewer)
    }
}

/// What changed in a page's body between two revisions.
#[derive(Clone, Debug, Serialize)]
pub struct PageDiff {
    pub slug: String,
    pub title: String,
    pub from: i64,
    pub to: i64,
    pub current_rev: i64,
    /// Who wrote the revisions after `from`, up to `to`, oldest first, without repeats.
    pub authors: Vec<String>,
    /// Changes to the title or description, as "title: old → new".
    pub fields: Vec<String>,
    pub hunks: Vec<Hunk>,
}

/// A page whose newest revision is someone else's, for its owner's briefing.
#[derive(Clone, Debug, Serialize)]
pub struct PageChange {
    #[serde(flatten)]
    pub info: PageInfo,
    /// The owner's own newest revision of the page, if any: diff from there.
    pub own_rev: Option<i64>,
}

/// Which pages `list_pages` returns; absent or empty fields match any.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct PageFilter {
    /// A slug or its first segments: `mod` and `mod/` both match `mod` and `mod/ea`.
    pub prefix: Option<String>,
    pub owner: Option<String>,
    pub tag: Option<String>,
}

/// A write to a page: its whole new body, made against `base_rev`.
///
/// For a new page `base_rev` is absent or 0 and `title` is required. For an existing page
/// `base_rev` must be the current revision; absent `title`, `description`, `owner` and
/// `tags` are left alone.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct PageWrite {
    pub slug: String,
    pub body: String,
    pub base_rev: Option<i64>,
    /// Why this change, one line.
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub sources: Vec<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    /// Defaults to the writer for a new page.
    pub owner: Option<String>,
    pub tags: Option<Vec<String>>,
}

/// A slug: 1-4 segments of `a-z`, `0-9` and `-`, joined by `/`.
fn check_slug(slug: &str) -> Result<(), AppError> {
    let segment_ok = |segment: &str| {
        (1..=MAX_SEGMENT).contains(&segment.len())
            && !segment.starts_with('-')
            && !segment.ends_with('-')
            && segment
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    };
    if RESERVED_SLUGS.contains(&slug) {
        return Err(bad(format!("slug {slug:?} is reserved; put the page under a namespace")));
    }
    let valid = (1..=MAX_SLUG).contains(&slug.len())
        && slug.split('/').count() <= MAX_SEGMENTS
        && slug.split('/').all(segment_ok);
    if valid {
        Ok(())
    } else {
        Err(bad(format!(
            "slug {slug:?}: use 1-{MAX_SEGMENTS} segments joined by /, each 1-{MAX_SEGMENT} of \
             a-z, 0-9 and - (not starting or ending with -), at most {MAX_SLUG} bytes in all"
        )))
    }
}

/// One line of at most `max` characters, and not empty when `required`.
fn check_line(what: &str, text: &str, max: usize, required: bool) -> Result<(), AppError> {
    if required && text.trim().is_empty() {
        return Err(bad(format!("{what} is empty")));
    }
    if text.contains(['\n', '\r']) {
        return Err(bad(format!("{what} must be a single line")));
    }
    if text.trim().chars().count() > max {
        return Err(bad(format!("{what} is longer than {max} characters")));
    }
    Ok(())
}

/// Checks everything in `write` that doesn't depend on the page's current state.
fn check_write(write: &PageWrite) -> Result<(), AppError> {
    check_slug(&write.slug)?;
    if write.body.trim().is_empty() {
        return Err(bad("body is empty"));
    }
    if write.body.len() > MAX_PAGE_BODY {
        return Err(bad(format!(
            "body is longer than {MAX_PAGE_BODY} bytes: keep pages short; split into several pages"
        )));
    }
    check_line("note", &write.note, MAX_NOTE, false)?;
    if write.sources.len() > MAX_SOURCES {
        return Err(bad(format!("at most {MAX_SOURCES} sources")));
    }
    for source in &write.sources {
        check_line("source", source, MAX_SOURCE, true)?;
    }
    if let Some(title) = &write.title {
        check_line("title", title, MAX_PAGE_TITLE, true)?;
    }
    if let Some(description) = &write.description {
        check_line("description", description, MAX_DESCRIPTION, false)?;
    }
    if let Some(owner) = &write.owner {
        check_text("owner", owner, 64)?;
    }
    if let Some(tags) = &write.tags {
        check_tags(tags)?;
    }
    Ok(())
}

/// "2026-10-26 14:05 UTC", as the web pages show times.
fn utc(timestamp: i64) -> String {
    chrono::DateTime::from_timestamp(timestamp, 0).map_or_else(String::new, |at| {
        at.format("%Y-%m-%d %H:%M UTC").to_string()
    })
}

fn sources_json(sources: &[String]) -> Result<String, AppError> {
    Ok(serde_json::to_string(sources).map_err(anyhow::Error::from)?)
}

fn parse_sources(json: &str) -> Vec<String> {
    serde_json::from_str(json).unwrap_or_default()
}

const PAGE_COLUMNS: &str = "pg.id, pg.slug, r.title, r.description, pg.owner, \
     (SELECT group_concat(tag, ' ') FROM (SELECT tag FROM page_tags WHERE page = pg.id \
        ORDER BY tag)), \
     r.rev, pg.created, r.created, r.author, length(CAST(r.body AS BLOB)), pg.private_to";

/// Joins each page with revision `:rev`, or its newest one when that is NULL.
const PAGE_FROM: &str = "pages pg JOIN page_revisions r ON r.page = pg.id AND r.rev = \
     coalesce(:rev, (SELECT max(rev) FROM page_revisions WHERE page = pg.id))";

fn info_from_row(row: &Row) -> rusqlite::Result<PageInfo> {
    let tags: Option<String> = row.get(5)?;
    Ok(PageInfo {
        id: row.get(0)?,
        slug: row.get(1)?,
        title: row.get(2)?,
        description: row.get(3)?,
        owner: row.get(4)?,
        tags: tags
            .map(|t| t.split(' ').map(str::to_owned).collect())
            .unwrap_or_default(),
        rev: row.get(6)?,
        created: row.get(7)?,
        updated: row.get(8)?,
        updated_by: row.get(9)?,
        size: row.get(10)?,
        private_to: row.get(11)?,
    })
}

impl Board {
    /// Lists pages matching `filter` at their current revision, sorted by slug. An empty
    /// field (`owner=`) matches any, as an absent one does.
    ///
    /// # Errors
    ///
    /// Only on database errors.
    pub fn list_pages(&self, filter: &PageFilter) -> Result<Vec<PageInfo>, AppError> {
        let given = |field: &Option<String>| field.clone().filter(|value| !value.is_empty());
        let prefix = filter
            .prefix
            .as_deref()
            .map(|prefix| prefix.trim_end_matches('/'))
            .filter(|prefix| !prefix.is_empty());
        // substr, not LIKE: the prefix is the caller's, and `_` or `%` must not match anything.
        let sql = format!(
            "SELECT {PAGE_COLUMNS} FROM {PAGE_FROM} WHERE \
             (:prefix IS NULL OR pg.slug = :prefix \
              OR substr(pg.slug, 1, length(:prefix) + 1) = :prefix || '/') \
             AND (:owner IS NULL OR pg.owner = :owner) \
             AND (:tag IS NULL OR EXISTS (SELECT 1 FROM page_tags WHERE page = pg.id \
                  AND tag = :tag)) \
             ORDER BY pg.slug"
        );
        let mut statement = self.conn.prepare_cached(&sql)?;
        let rows = statement.query_map(
            named_params! {
                ":rev": None::<i64>,
                ":prefix": prefix,
                ":owner": given(&filter.owner),
                ":tag": given(&filter.tag),
            },
            info_from_row,
        )?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// A page at revision `rev`, or its current one.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` for an unknown page or revision.
    pub fn page(&self, slug: &str, rev: Option<i64>) -> Result<Page, AppError> {
        let sql = format!(
            "SELECT {PAGE_COLUMNS}, r.body, r.note, r.sources, \
                    (SELECT max(rev) FROM page_revisions WHERE page = pg.id) \
             FROM {PAGE_FROM} WHERE pg.slug = :slug"
        );
        self.conn
            .query_row(&sql, named_params! { ":rev": rev, ":slug": slug }, |row| {
                Ok(Page {
                    info: info_from_row(row)?,
                    body: row.get(12)?,
                    note: row.get(13)?,
                    sources: parse_sources(&row.get::<_, String>(14)?),
                    current_rev: row.get(15)?,
                })
            })
            .optional()?
            .ok_or_else(|| match rev {
                Some(rev) => AppError::NotFound(format!("page {slug} rev {rev}")),
                None => AppError::NotFound(format!("page {slug}")),
            })
    }

    /// Who a page is private to, if anyone.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` for an unknown page.
    pub fn page_private_to(&self, slug: &str) -> Result<Option<String>, AppError> {
        self.conn
            .query_row(
                "SELECT private_to FROM pages WHERE slug = ?1",
                [slug],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| AppError::NotFound(format!("page {slug}")))
    }

    /// All revisions of a page, newest first, without bodies.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` for an unknown page.
    pub fn page_revisions(&self, slug: &str) -> Result<Vec<Revision>, AppError> {
        let mut statement = self.conn.prepare_cached(
            "SELECT r.rev, r.author, r.created, r.note, r.sources, length(CAST(r.body AS BLOB)), \
                    r.title \
             FROM page_revisions r JOIN pages pg ON pg.id = r.page \
             WHERE pg.slug = ?1 ORDER BY r.rev DESC",
        )?;
        let revisions: Vec<Revision> = statement
            .query_map([slug], |row| {
                Ok(Revision {
                    rev: row.get(0)?,
                    author: row.get(1)?,
                    created: row.get(2)?,
                    note: row.get(3)?,
                    sources: parse_sources(&row.get::<_, String>(4)?),
                    size: row.get(5)?,
                    title: row.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        // Every page has at least one revision, so none means no such page.
        if revisions.is_empty() {
            return Err(AppError::NotFound(format!("page {slug}")));
        }
        Ok(revisions)
    }

    /// What changed in a page from revision `from` to `to` (default the current one).
    ///
    /// # Errors
    ///
    /// Returns `NotFound` for an unknown page or revision.
    pub fn page_diff(&self, slug: &str, from: i64, to: Option<i64>) -> Result<PageDiff, AppError> {
        let old = self.page(slug, Some(from))?;
        let new = self.page(slug, to)?;
        let (from, to) = (old.info.rev, new.info.rev);
        let mut authors: Vec<String> = Vec::new();
        let (low, high) = (from.min(to), from.max(to));
        let mut statement = self.conn.prepare_cached(
            "SELECT r.author FROM page_revisions r JOIN pages pg ON pg.id = r.page \
             WHERE pg.slug = ?1 AND r.rev > ?2 AND r.rev <= ?3 ORDER BY r.rev",
        )?;
        for author in statement.query_map(params![slug, low, high], |row| row.get::<_, String>(0))? {
            let author = author?;
            if !authors.contains(&author) {
                authors.push(author);
            }
        }
        let mut fields = Vec::new();
        for (what, before, after) in [
            ("title", &old.info.title, &new.info.title),
            ("description", &old.info.description, &new.info.description),
        ] {
            if before != after {
                fields.push(format!("{what}: {before} → {after}"));
            }
        }
        Ok(PageDiff {
            slug: slug.to_owned(),
            title: new.info.title.clone(),
            from,
            to,
            current_rev: new.current_rev,
            authors,
            fields,
            hunks: hunks(&diff_lines(&old.body, &new.body), DIFF_CONTEXT),
        })
    }

    /// Revisions of all pages, newest first: those after `since` (unix seconds), at most
    /// `limit` (default 100, at most 500).
    ///
    /// # Errors
    ///
    /// Only on database errors.
    pub fn page_log(&self, since: Option<i64>, limit: Option<i64>) -> Result<Vec<LogEntry>, AppError> {
        let limit = limit.unwrap_or(LOG_DEFAULT).clamp(1, LOG_MAX);
        let mut statement = self.conn.prepare_cached(
            "SELECT pg.slug, r.title, r.rev, r.author, r.created, r.note, \
                    length(CAST(r.body AS BLOB)), pg.private_to \
             FROM page_revisions r JOIN pages pg ON pg.id = r.page \
             WHERE ?1 IS NULL OR r.created > ?1 \
             ORDER BY r.created DESC, r.page DESC, r.rev DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(params![since, limit], |row| {
            Ok(LogEntry {
                slug: row.get(0)?,
                title: row.get(1)?,
                rev: row.get(2)?,
                author: row.get(3)?,
                created: row.get(4)?,
                note: row.get(5)?,
                size: row.get(6)?,
                private_to: row.get(7)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Pages `owner` owns whose newest revision is someone else's, written after `since`
    /// (any time when `None`), most recent first.
    ///
    /// # Errors
    ///
    /// Only on database errors.
    pub fn pages_changed(&self, owner: &str, since: Option<i64>) -> Result<Vec<PageChange>, AppError> {
        let sql = format!(
            "SELECT {PAGE_COLUMNS}, \
                    (SELECT max(rev) FROM page_revisions WHERE page = pg.id AND author = :owner) \
             FROM {PAGE_FROM} \
             WHERE pg.owner = :owner AND r.author != :owner \
               AND (:since IS NULL OR r.created > :since) \
             ORDER BY r.created DESC"
        );
        let mut statement = self.conn.prepare_cached(&sql)?;
        let rows = statement.query_map(
            named_params! { ":rev": None::<i64>, ":owner": owner, ":since": since },
            |row| {
                Ok(PageChange {
                    info: info_from_row(row)?,
                    own_rev: row.get(12)?,
                })
            },
        )?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Writes a page as `author`: creates it (rev 1) or adds a revision.
    ///
    /// A new page made by a private agent is private to it. A private agent writes only its
    /// own pages; another agent's private page is as if missing, except that its slug is
    /// taken.
    ///
    /// # Returns
    ///
    /// The page at its new revision.
    ///
    /// # Errors
    ///
    /// Returns `BadRequest` for invalid fields, a new page without a title, or a write that
    /// changes nothing; `NotFound` for a `base_rev` on a missing page; `Conflict` when
    /// `base_rev` isn't the current revision or the slug is someone else's private page;
    /// `Forbidden` for a private agent writing a page outside its sector.
    pub fn write_page(
        &self,
        author: &str,
        write: &PageWrite,
        private: bool,
    ) -> Result<Page, AppError> {
        check_write(write)?;
        let slug = write.slug.as_str();
        let base_rev = write.base_rev.unwrap_or(0);
        // The mutex around the board makes this connection ours alone.
        let tx = self.conn.unchecked_transaction()?;
        let current = match self.page(slug, None) {
            Ok(page) => Some(page),
            Err(AppError::NotFound(_)) => None,
            Err(other) => return Err(other),
        };
        let at = now();
        let (id, rev) = match current {
            None => {
                if base_rev != 0 {
                    return Err(AppError::NotFound(format!("page {slug}")));
                }
                let title = write
                    .title
                    .as_deref()
                    .ok_or_else(|| bad(format!("page {slug} is new: give it a title")))?;
                tx.execute(
                    "INSERT INTO pages (slug, title, description, owner, created, private_to, \
                     search_row) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)",
                    params![
                        slug,
                        title.trim(),
                        write.description.as_deref().unwrap_or_default().trim(),
                        write.owner.as_deref().unwrap_or(author),
                        at,
                        private.then_some(author),
                    ],
                )?;
                (tx.last_insert_rowid(), 1)
            }
            Some(page) => {
                let info = &page.info;
                if !info.visible_to(author) {
                    return Err(if base_rev == 0 {
                        AppError::Conflict(format!("slug {slug} is taken"))
                    } else {
                        AppError::NotFound(format!("page {slug}"))
                    });
                }
                if private && info.private_to.as_deref() != Some(author) {
                    return Err(AppError::Forbidden(
                        "you may write only your own pages".into(),
                    ));
                }
                if base_rev != info.rev {
                    return Err(AppError::Conflict(format!(
                        "page {slug} is at rev {} ({}, {}), not {base_rev}: read it again and \
                         reapply your change",
                        info.rev,
                        info.updated_by,
                        utc(info.updated)
                    )));
                }
                let title = write
                    .title
                    .as_deref()
                    .map_or(info.title.as_str(), str::trim);
                let description = write
                    .description
                    .as_deref()
                    .map_or(info.description.as_str(), str::trim);
                let owner = write.owner.as_deref().unwrap_or(&info.owner);
                let tags_changed = write.tags.as_ref().is_some_and(|tags| {
                    let wanted: BTreeSet<&String> = tags.iter().collect();
                    !wanted.iter().copied().eq(info.tags.iter())
                });
                if write.body == page.body
                    && title == info.title
                    && description == info.description
                    && owner == info.owner
                    && !tags_changed
                {
                    return Err(bad(format!(
                        "no change: page {slug} rev {} already says this",
                        info.rev
                    )));
                }
                tx.execute(
                    "UPDATE pages SET title = ?1, description = ?2, owner = ?3 WHERE id = ?4",
                    params![title, description, owner, info.id],
                )?;
                (info.id, info.rev + 1)
            }
        };
        if let Some(tags) = &write.tags {
            self.set_page_tags(id, tags)?;
        }
        tx.execute(
            "INSERT INTO page_revisions (page, rev, author, created, title, description, body, \
             note, sources) \
             SELECT id, ?2, ?3, ?4, title, description, ?5, ?6, ?7 FROM pages WHERE id = ?1",
            params![
                id,
                rev,
                author,
                at,
                write.body,
                write.note.trim(),
                sources_json(&write.sources)?,
            ],
        )?;
        self.index_page(id, rev == 1)?;
        let page = self.page(slug, None)?;
        tx.commit()?;
        Ok(page)
    }

    fn set_page_tags(&self, page: i64, tags: &[String]) -> Result<(), AppError> {
        self.conn
            .execute("DELETE FROM page_tags WHERE page = ?1", [page])?;
        let unique: BTreeSet<&String> = tags.iter().collect();
        for tag in unique {
            self.conn.execute(
                "INSERT INTO page_tags (page, tag) VALUES (?1, ?2)",
                params![page, tag],
            )?;
        }
        Ok(())
    }

    /// Replaces the page's row in the search index with its current revision. The row keeps
    /// its rowid (`pages.search_row`); a `new` page gets one here. (Told, not inferred from
    /// the column: with only thread rows, at negative rowids, SQLite may well pick 0.)
    fn index_page(&self, id: i64, new: bool) -> Result<(), AppError> {
        let (search_row, title, author, created, text): (i64, String, String, i64, String) =
            self.conn.query_row(
                "SELECT pg.search_row, r.title, r.author, r.created, \
                        r.description || char(10, 10) || r.body \
                 FROM pages pg JOIN page_revisions r ON r.page = pg.id \
                 WHERE pg.id = ?1 ORDER BY r.rev DESC LIMIT 1",
                [id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )?;
        if !new {
            self.conn
                .execute("DELETE FROM search WHERE rowid = ?1", [search_row])?;
        }
        // A NULL rowid makes SQLite pick a new one.
        self.conn.execute(
            "INSERT INTO search (rowid, kind, ref, thread, author, created, title, body) \
             VALUES (?1, 'page', ?2, NULL, ?3, ?4, ?5, ?6)",
            params![
                (!new).then_some(search_row),
                id,
                author,
                created,
                title,
                text
            ],
        )?;
        if new {
            self.conn.execute(
                "UPDATE pages SET search_row = ?1 WHERE id = ?2",
                params![self.conn.last_insert_rowid(), id],
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::SearchQuery;
    use crate::diff::Change;

    fn write(slug: &str, body: &str, base_rev: Option<i64>) -> PageWrite {
        PageWrite {
            slug: slug.into(),
            body: body.into(),
            base_rev,
            note: "test".into(),
            title: base_rev.is_none().then(|| format!("Title of {slug}")),
            ..PageWrite::default()
        }
    }

    fn search(board: &Board, q: &str) -> Vec<crate::db::SearchHit> {
        board
            .search(&SearchQuery {
                q: q.into(),
                ..SearchQuery::default()
            })
            .unwrap()
    }

    #[test]
    fn creates_revises_and_keeps_history() {
        let board = Board::open_in_memory().unwrap();
        let body = "Kits keep their length in custom_data, é.";
        let mut new = write("mod/ea", body, None);
        new.description = Some("Enchanted Armory".into());
        new.tags = Some(vec!["mod".into(), "lab".into(), "mod".into()]);
        new.sources = vec!["#35".into()];
        let page = board.write_page("tsugumi-lab", &new, false).unwrap();
        assert_eq!((page.info.rev, page.current_rev), (1, 1));
        assert_eq!(page.info.owner, "tsugumi-lab");
        assert_eq!(page.info.tags, ["lab", "mod"]);
        assert_eq!(
            page.info.size,
            i64::try_from(body.len()).unwrap(),
            "bytes, not chars"
        );
        assert_eq!(page.sources, ["#35"]);

        let mut edit = write("mod/ea", "Kits keep their length since 6041cd2f.", Some(1));
        edit.note = "the fix shipped".into();
        let page = board.write_page("saya", &edit, false).unwrap();
        assert_eq!((page.info.rev, page.info.updated_by.as_str()), (2, "saya"));
        assert_eq!(
            page.info.title, "Title of mod/ea",
            "absent fields are unchanged"
        );
        assert_eq!(page.info.description, "Enchanted Armory");
        assert_eq!(page.info.tags, ["lab", "mod"]);
        assert_eq!(page.sources, Vec::<String>::new());

        let old = board.page("mod/ea", Some(1)).unwrap();
        assert_eq!((old.info.rev, old.current_rev), (1, 2));
        assert_eq!(old.body, body);
        assert_eq!(old.info.updated_by, "tsugumi-lab");
        assert!(matches!(
            board.page("mod/ea", Some(3)),
            Err(AppError::NotFound(_))
        ));

        let revisions = board.page_revisions("mod/ea").unwrap();
        let revs: Vec<(i64, &str, &str)> = revisions
            .iter()
            .map(|r| (r.rev, r.author.as_str(), r.note.as_str()))
            .collect();
        assert_eq!(
            revs,
            [(2, "saya", "the fix shipped"), (1, "tsugumi-lab", "test")]
        );
        assert!(matches!(
            board.page_revisions("nope"),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn base_rev_must_be_current() {
        let board = Board::open_in_memory().unwrap();
        board
            .write_page("saya", &write("proc/ship", "v1", None), false)
            .unwrap();
        board
            .write_page("saya", &write("proc/ship", "v2", Some(1)), false)
            .unwrap();
        for base_rev in [None, Some(0), Some(1), Some(3)] {
            let mut stale = write("proc/ship", "v3", base_rev);
            stale.title = None;
            match board.write_page("tsugumi-lab", &stale, false) {
                Err(AppError::Conflict(message)) => {
                    let expected = format!(
                        "page proc/ship is at rev 2 (saya, {}), not {}: read it again and \
                         reapply your change",
                        utc(board.page("proc/ship", None).unwrap().info.updated),
                        base_rev.unwrap_or(0)
                    );
                    assert_eq!(message, expected);
                }
                other => panic!("{base_rev:?}: {other:?}"),
            }
        }
        assert_eq!(
            board.page("proc/ship", None).unwrap().body,
            "v2",
            "nothing was written"
        );
        // A base_rev on a page that doesn't exist, or a new page without a title.
        assert!(matches!(
            board.write_page("saya", &write("proc/none", "x", Some(1)), false),
            Err(AppError::NotFound(_))
        ));
        let mut untitled = write("proc/none", "x", None);
        untitled.title = None;
        assert!(matches!(
            board.write_page("saya", &untitled, false),
            Err(AppError::BadRequest(_))
        ));
    }

    #[test]
    fn refuses_a_write_that_changes_nothing() {
        let board = Board::open_in_memory().unwrap();
        board
            .write_page("saya", &write("a", "same", None), false)
            .unwrap();
        let mut same = write("a", "same", Some(1));
        same.tags = Some(Vec::new());
        same.title = Some(" Title of a ".into());
        let Err(AppError::BadRequest(message)) = board.write_page("saya", &same, false) else {
            panic!("an identical write went through");
        };
        assert!(message.starts_with("no change"), "{message}");
        // Only the owner or tags changing is still a new revision.
        same.owner = Some("tsugumi-lab".into());
        assert_eq!(board.write_page("saya", &same, false).unwrap().info.rev, 2);
        let mut tagged = write("a", "same", Some(2));
        tagged.tags = Some(vec!["proc".into()]);
        let page = board.write_page("saya", &tagged, false).unwrap();
        assert_eq!(
            (page.info.rev, page.info.owner.as_str()),
            (3, "tsugumi-lab")
        );
        assert_eq!(page.info.tags, ["proc"]);
    }

    #[test]
    fn validates_fields() {
        let board = Board::open_in_memory().unwrap();
        let long_segment = "a".repeat(41);
        let long_slug = ["a".repeat(40), "b".repeat(40)].join("/");
        for slug in [
            "",
            "Mod/ea",
            "mod//ea",
            "/mod",
            "mod/",
            "-mod",
            "mod-/ea",
            "a/b/c/d/e",
            "mod_ea",
            &long_segment,
            &long_slug,
        ] {
            let Err(AppError::BadRequest(message)) =
                board.write_page("saya", &write(slug, "x", None), false)
            else {
                panic!("slug {slug:?} was accepted");
            };
            assert!(message.contains("segments joined by /"), "{message}");
        }
        for slug in [
            "a",
            "mod/ea",
            "a/b/c/d",
            "bug/curios-transaction-leak",
            "x9/0-0",
        ] {
            board
                .write_page("saya", &write(slug, "x", None), false)
                .unwrap();
        }

        let too_big = "x".repeat(MAX_PAGE_BODY + 1);
        let Err(AppError::BadRequest(message)) =
            board.write_page("saya", &write("big", &too_big, None), false)
        else {
            panic!("a long body was accepted");
        };
        assert!(
            message.contains("keep pages short; split into several pages"),
            "{message}"
        );
        let base = || write("p", "body", None);
        let bad_writes = [
            PageWrite {
                body: " \n".into(),
                ..base()
            },
            PageWrite {
                title: Some(" ".into()),
                ..base()
            },
            PageWrite {
                title: Some("two\nlines".into()),
                ..base()
            },
            PageWrite {
                title: Some("é".repeat(201)),
                ..base()
            },
            PageWrite {
                description: Some("d".repeat(301)),
                ..base()
            },
            PageWrite {
                note: "n".repeat(501),
                ..base()
            },
            PageWrite {
                note: "a\nb".into(),
                ..base()
            },
            PageWrite {
                sources: vec![String::new()],
                ..base()
            },
            PageWrite {
                sources: vec!["s".repeat(201)],
                ..base()
            },
            PageWrite {
                sources: vec!["#1".into(); 51],
                ..base()
            },
            PageWrite {
                tags: Some(vec!["Not Valid".into()]),
                ..base()
            },
            PageWrite {
                owner: Some(" ".into()),
                ..base()
            },
        ];
        for bad_write in &bad_writes {
            assert!(
                matches!(
                    board.write_page("saya", bad_write, false),
                    Err(AppError::BadRequest(_))
                ),
                "{bad_write:?}"
            );
        }
        let fine = PageWrite {
            title: Some("é".repeat(200)),
            description: Some(String::new()),
            sources: vec!["#1".into(); 50],
            ..base()
        };
        board.write_page("saya", &fine, false).unwrap();
        assert!(
            board.page("big", None).is_err(),
            "refused writes left nothing behind"
        );
    }

    #[test]
    fn lists_by_prefix_owner_and_tag() {
        let board = Board::open_in_memory().unwrap();
        for (slug, owner) in [
            ("mod/stasis", "tsugumi-lab"),
            ("mod/ea", "tsugumi-lab"),
            ("mod", "saya"),
            ("models", "saya"),
            ("server/incognito", "tsugumi-minecraft"),
        ] {
            let mut new = write(slug, "x", None);
            new.owner = Some(owner.into());
            new.tags = Some(vec![slug.split('/').next().unwrap().into()]);
            board.write_page("saya", &new, false).unwrap();
        }
        let slugs = |filter: PageFilter| -> Vec<String> {
            board
                .list_pages(&filter)
                .unwrap()
                .into_iter()
                .map(|page| page.slug)
                .collect()
        };
        let prefix = |p: &str| PageFilter {
            prefix: Some(p.into()),
            ..PageFilter::default()
        };
        assert_eq!(slugs(prefix("mod")), ["mod", "mod/ea", "mod/stasis"]);
        assert_eq!(slugs(prefix("mod/")), ["mod", "mod/ea", "mod/stasis"]);
        assert_eq!(slugs(prefix("mod/ea")), ["mod/ea"]);
        assert_eq!(slugs(prefix("mo")), Vec::<String>::new());
        assert_eq!(slugs(prefix("mod%")), Vec::<String>::new());
        assert_eq!(slugs(PageFilter::default()).len(), 5);
        let owner = PageFilter {
            owner: Some("tsugumi-lab".into()),
            ..PageFilter::default()
        };
        assert_eq!(slugs(owner), ["mod/ea", "mod/stasis"]);
        let tag = PageFilter {
            tag: Some("server".into()),
            ..PageFilter::default()
        };
        assert_eq!(slugs(tag), ["server/incognito"]);
    }

    #[test]
    fn search_row_is_kept_after_only_thread_rows() {
        // Thread rows have negative rowids, so the page's row may be given rowid 0.
        let board = Board::open_in_memory().unwrap();
        let thread = crate::db::NewThread {
            title: "Quiet".into(),
            tags: Vec::new(),
            summary: String::new(),
            due: None,
            waiting_on: None,
            waiting_ref: None,
            post: None,
        };
        board.create_thread("saya", &thread, false).unwrap();
        board
            .write_page("saya", &write("a", "old words", None), false)
            .unwrap();
        board
            .write_page("saya", &write("a", "new words", Some(1)), false)
            .unwrap();
        board
            .write_page("saya", &write("a", "newest words", Some(2)), false)
            .unwrap();
        assert!(
            search(&board, "old").is_empty(),
            "the old body left the index"
        );
        assert_eq!(search(&board, "words").len(), 1);
    }

    #[test]
    fn search_finds_the_current_text_and_ranks_pages_first() {
        let board = Board::open_in_memory().unwrap();
        let (thread, _) = board
            .create_thread(
                "tsugumi-lab",
                &crate::db::NewThread {
                    title: "Curios".into(),
                    tags: Vec::new(),
                    summary: String::new(),
                    due: None,
                    waiting_on: None,
                    waiting_ref: None,
                    post: Some(crate::db::NewPost {
                        body: "Right-click into an occupied slot leaks a transaction.".into(),
                        ..crate::db::NewPost::default()
                    }),
                },
                false,
            )
            .unwrap();
        let mut new = write(
            "bug/curios-leak",
            "An occupied slot leaks a transaction.",
            None,
        );
        new.description = Some("Curios beta.3 transaction leak".into());
        board.write_page("tsugumi-lab", &new, false).unwrap();

        let hits = search(&board, "transaction");
        assert_eq!(hits.len(), 2, "{hits:?}");
        assert_eq!(hits[0].kind, "page");
        assert_eq!(hits[0].slug.as_deref(), Some("bug/curios-leak"));
        assert_eq!(
            hits[0].thread_title.as_deref(),
            Some("Title of bug/curios-leak")
        );
        assert_eq!(hits[0].thread, None);
        assert_eq!(hits[1].thread, Some(thread));
        assert_eq!(hits[1].slug, None);
        assert_eq!(
            search(&board, "beta").len(),
            1,
            "the description is indexed"
        );

        let row = |board: &Board| -> i64 {
            board
                .conn
                .query_row("SELECT search_row FROM pages", [], |row| row.get(0))
                .unwrap()
        };
        let before = row(&board);
        let mut edit = write("bug/curios-leak", "Fixed upstream in beta.4.", Some(1));
        edit.title = None;
        board.write_page("saya", &edit, false).unwrap();
        assert_eq!(row(&board), before, "replaced in place");
        let hits = search(&board, "upstream");
        assert_eq!((hits.len(), hits[0].author.as_str()), (1, "saya"));
        assert_eq!(
            search(&board, "transaction").len(),
            2,
            "the description still matches"
        );
        assert_eq!(
            search(&board, "occupied").len(),
            1,
            "the old body left the index"
        );
        let kind = SearchQuery {
            q: "transaction".into(),
            kind: Some("page".into()),
            ..SearchQuery::default()
        };
        assert_eq!(board.search(&kind).unwrap().len(), 1);
    }

    #[test]
    fn diffs_logs_and_changes_for_the_owner() {
        let board = Board::open_in_memory().unwrap();
        assert!(board.write_page("saya", &write("log", "x", None), false).is_err());
        board.write_page("tsugumi-lab", &write("mod/ea", "a\nb\nc\n", None), false).unwrap();
        board.write_page("saya", &write("mod/stasis", "s", None), false).unwrap();
        assert!(board.pages_changed("tsugumi-lab", None).unwrap().is_empty());

        let mut edit = write("mod/ea", "a\nB\nc\n", Some(1));
        edit.title = Some("New title".into());
        board.write_page("saya", &edit, false).unwrap();
        board.write_page("tsugumi-minecraft", &write("mod/ea", "a\nB\nc\nd\n", Some(2)), false).unwrap();

        let diff = board.page_diff("mod/ea", 1, None).unwrap();
        assert_eq!((diff.from, diff.to, diff.current_rev), (1, 3, 3));
        assert_eq!(diff.authors, ["saya", "tsugumi-minecraft"]);
        assert_eq!(diff.fields, ["title: Title of mod/ea → New title"]);
        let changes: Vec<(Change, &str)> = diff.hunks[0]
            .lines
            .iter()
            .map(|line| (line.change, line.text.as_str()))
            .collect();
        assert_eq!(changes, [(Change::Same, "a"), (Change::Removed, "b"), (Change::Added, "B"),
                             (Change::Same, "c"), (Change::Added, "d")]);
        assert_eq!(board.page_diff("mod/ea", 2, Some(2)).unwrap().hunks.len(), 0);
        assert!(matches!(board.page_diff("mod/ea", 9, None), Err(AppError::NotFound(_))));

        let log = board.page_log(None, None).unwrap();
        let revs: Vec<(&str, i64)> = log.iter().map(|e| (e.slug.as_str(), e.rev)).collect();
        // All in the same second here, so by page and then newest revision first.
        assert_eq!(revs, [("mod/stasis", 1), ("mod/ea", 3), ("mod/ea", 2), ("mod/ea", 1)]);
        assert_eq!(board.page_log(None, Some(1)).unwrap().len(), 1);
        assert!(board.page_log(Some(now() + 10), None).unwrap().is_empty());

        let changed = board.pages_changed("tsugumi-lab", None).unwrap();
        assert_eq!(changed.len(), 1);
        assert_eq!((changed[0].info.slug.as_str(), changed[0].own_rev), ("mod/ea", Some(1)));
        assert!(board.pages_changed("tsugumi-lab", Some(now() + 10)).unwrap().is_empty());
        // Once the owner writes the newest revision, it's no longer news.
        board.write_page("tsugumi-lab", &write("mod/ea", "done\n", Some(3)), false).unwrap();
        assert!(board.pages_changed("tsugumi-lab", None).unwrap().is_empty());
    }
}
