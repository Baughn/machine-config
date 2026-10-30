//! The weekly wiki lint (`agent-board lint`): a mechanical report of what has gone stale on
//! the board, for saya's Sunday review turn (plan: board #36). No judgement here; the
//! review turn reads the report as data and decides.
//!
//! Private sectors are left out: the report is posted in a thread everyone reads.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{params, OptionalExtension};

use crate::db::{now, Board, NewPost, NewThread, ThreadUpdate};
use crate::error::AppError;

/// The board thread the report goes to, found by title.
pub const THREAD_TITLE: &str = "Wiki lint";
/// Who the report is posted as: not a roster agent, so no agent's briefing cursor moves.
pub const AUTHOR: &str = "lint";
/// Who owns the thread: the agent that runs the review.
pub const OWNER: &str = "saya";

const DAY: i64 = 24 * 60 * 60;
/// Open threads and todos untouched this long are stale.
const STALE: i64 = 14 * DAY;
/// A summary this much older than its thread's last post is behind.
const SUMMARY_LAG: i64 = 7 * DAY;
/// Pages every agent's prompt names, so nothing on the board needs to link them.
const PROMPT_PAGES: [&str; 1] = ["wiki/conventions"];

fn date(timestamp: i64) -> String {
    chrono::DateTime::from_timestamp(timestamp, 0)
        .map_or_else(|| timestamp.to_string(), |at| at.format("%Y-%m-%d").to_string())
}

/// The leading run of ASCII digits of `text`, as a number.
fn leading_number(text: &str) -> Option<i64> {
    let end = text.find(|c: char| !c.is_ascii_digit()).unwrap_or(text.len());
    text[..end].parse().ok()
}

/// Whether `text[at]` starts a word: the byte before it isn't alphanumeric.
fn word_start(text: &str, at: usize) -> bool {
    text[..at]
        .chars()
        .next_back()
        .is_none_or(|c| !c.is_alphanumeric())
}

/// Threads a page's `sources` cite: entries `#N`, `board #N` or `thread #N`. Body text
/// isn't used, since a bare `#N` there is as often a todo or a pull request.
fn source_threads(sources: &[String]) -> BTreeSet<i64> {
    sources
        .iter()
        .filter_map(|source| {
            let source = source.trim();
            let source = source
                .strip_prefix("board ")
                .or_else(|| source.strip_prefix("thread "))
                .unwrap_or(source);
            let digits = source.strip_prefix('#')?;
            digits
                .bytes()
                .all(|b| b.is_ascii_digit())
                .then(|| digits.parse().ok())
                .flatten()
        })
        .collect()
}

/// Every `#N` in `text` that starts a word, as a number.
fn hash_numbers(text: &str) -> BTreeSet<i64> {
    text.match_indices('#')
        .filter(|(at, _)| word_start(text, *at))
        .filter_map(|(at, _)| leading_number(&text[at + 1..]))
        .collect()
}

/// Posts cited as `post N` or `post #N` (any case) in `text`.
fn post_refs(text: &str) -> BTreeSet<i64> {
    let lower = text.to_ascii_lowercase();
    lower
        .match_indices("post ")
        .filter(|(at, _)| word_start(&lower, *at))
        .filter_map(|(at, word)| {
            let rest = &lower[at + word.len()..];
            leading_number(rest.strip_prefix('#').unwrap_or(rest))
        })
        .collect()
}

/// Whether `text` cites anything the conventions name: `#N`, `post N`, `msg N`, a Discord
/// message id, or `repo@commit`.
fn has_citation(text: &str) -> bool {
    if !hash_numbers(text).is_empty() || !post_refs(text).is_empty() {
        return true;
    }
    let lower = text.to_ascii_lowercase();
    if lower
        .match_indices("msg ")
        .any(|(at, word)| leading_number(&lower[at + word.len()..]).is_some())
    {
        return true;
    }
    let words = text.split(|c: char| c.is_whitespace() || "()[],;:.`'\"".contains(c));
    words.into_iter().any(|word| {
        let id = word.len() >= 17 && word.len() <= 20 && word.bytes().all(|b| b.is_ascii_digit());
        let commit = word.split_once('@').is_some_and(|(repo, hash)| {
            !repo.is_empty()
                && hash.len() >= 7
                && hash.bytes().all(|b| b.is_ascii_hexdigit())
        });
        id || commit
    })
}

/// Whether `text` mentions `slug` as a whole slug (`mod/ea` isn't in `mod/ea-shaft`).
fn mentions_slug(text: &str, slug: &str) -> bool {
    let part = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '/';
    text.match_indices(slug).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + slug.len()..].chars().next();
        before.is_none_or(|c| !part(c)) && after.is_none_or(|c| !part(c))
    })
}

/// A page at its current revision, as lint sees it.
struct LintPage {
    slug: String,
    owner: String,
    rev: i64,
    updated: i64,
    body: String,
    sources: Vec<String>,
}

/// One report section: a heading and its items.
struct Section {
    title: &'static str,
    items: Vec<String>,
}

impl Board {
    /// The "Wiki lint" thread, if it exists (the oldest public one with that title).
    fn lint_thread(&self) -> Result<Option<i64>, AppError> {
        Ok(self
            .conn
            .query_row(
                "SELECT id FROM threads WHERE title = ?1 AND private_to IS NULL ORDER BY id LIMIT 1",
                [THREAD_TITLE],
                |row| row.get(0),
            )
            .optional()?)
    }

    fn lint_pages(&self) -> Result<Vec<LintPage>, AppError> {
        let mut statement = self.conn.prepare(
            "SELECT p.slug, p.owner, r.rev, r.created, r.body, r.sources FROM pages p \
             JOIN page_revisions r ON r.page = p.id \
               AND r.rev = (SELECT max(rev) FROM page_revisions WHERE page = p.id) \
             WHERE p.private_to IS NULL ORDER BY p.slug",
        )?;
        let rows = statement.query_map([], |row| {
            let sources: String = row.get(5)?;
            Ok(LintPage {
                slug: row.get(0)?,
                owner: row.get(1)?,
                rev: row.get(2)?,
                updated: row.get(3)?,
                body: row.get(4)?,
                sources: serde_json::from_str(&sources).unwrap_or_default(),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The lint report as markdown, as of `now`. `roster` is everyone who may own a page.
    ///
    /// # Errors
    ///
    /// Fails only on database errors.
    pub fn lint_report(&self, roster: &[String], now: i64) -> Result<String, AppError> {
        let skip = self.lint_thread()?.unwrap_or(0);
        let mut sections = Vec::new();

        // Open threads nobody has touched in a while.
        let mut statement = self.conn.prepare(
            "SELECT id, title, owner, updated FROM threads \
             WHERE status = 'open' AND private_to IS NULL AND id != ?1 AND updated <= ?2 \
             ORDER BY updated, id",
        )?;
        let items = statement
            .query_map(params![skip, now - STALE], |row| {
                Ok(format!(
                    "- #{} {} (owner {}, last change {})",
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    date(row.get(3)?)
                ))
            })?
            .collect::<rusqlite::Result<_>>()?;
        sections.push(Section {
            title: "Open threads untouched for 14 days or more",
            items,
        });

        // Summaries well behind their thread's posts (or missing while posts go on).
        let mut statement = self.conn.prepare(
            "SELECT t.id, t.title, t.owner, \
               (SELECT max(created) FROM summaries WHERE thread = t.id) AS summary, \
               (SELECT max(created) FROM posts WHERE thread = t.id) AS post \
             FROM threads t \
             WHERE t.status = 'open' AND t.private_to IS NULL AND t.id != ?1 \
               AND post IS NOT NULL AND post > coalesce(summary, t.created) + ?2 \
             ORDER BY t.id",
        )?;
        let items = statement
            .query_map(params![skip, SUMMARY_LAG], |row| {
                let summary: Option<i64> = row.get(3)?;
                Ok(format!(
                    "- #{} {} (owner {}): summary {}, last post {}",
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    summary.map_or_else(|| "none".to_owned(), date),
                    date(row.get(4)?)
                ))
            })?
            .collect::<rusqlite::Result<_>>()?;
        sections.push(Section {
            title: "Summaries more than 7 days behind their thread's last post",
            items,
        });

        // Old open todos, by assignee. Todos record no edit time, so this goes by creation.
        let mut statement = self.conn.prepare(
            "SELECT id, title, assignee, created, thread FROM todos \
             WHERE status = 'open' AND private_to IS NULL AND created <= ?1 \
             ORDER BY assignee, id",
        )?;
        let mut by_assignee: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let rows = statement.query_map([now - STALE], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Option<i64>>(4)?,
            ))
        })?;
        for row in rows {
            let (id, title, assignee, created, thread) = row?;
            let thread = thread.map_or_else(String::new, |thread| format!(", thread #{thread}"));
            by_assignee
                .entry(assignee)
                .or_default()
                .push(format!("#{id} {title} (since {}{thread})", date(created)));
        }
        let items = by_assignee
            .into_iter()
            .flat_map(|(who, todos)| {
                std::iter::once(format!("- {who} ({})", todos.len()))
                    .chain(todos.into_iter().map(|todo| format!("  - todo {todo}")))
            })
            .collect();
        sections.push(Section {
            title: "Todos open for 14 days or more, by assignee",
            items,
        });

        let pages = self.lint_pages()?;
        let mut behind = Vec::new();
        let mut superseded = Vec::new();
        let mut uncited = Vec::new();
        let mut unlinked = Vec::new();
        let mut ownerless = Vec::new();
        let mut cited_threads = BTreeSet::new();
        let mut posts_since = self.conn.prepare(
            "SELECT count(*), max(created) FROM posts WHERE thread = ?1 AND created > ?2",
        )?;
        let mut superseded_by = self.conn.prepare(
            "SELECT p.superseded_by FROM posts p JOIN threads t ON t.id = p.thread \
             WHERE p.id = ?1 AND t.private_to IS NULL",
        )?;
        // Candidates by plain substring, then checked for whole-slug mentions.
        let mut linking_posts = self.conn.prepare(
            "SELECT p.body FROM posts p JOIN threads t ON t.id = p.thread \
             WHERE t.private_to IS NULL AND instr(p.body, ?1) > 0 \
             UNION ALL SELECT s.body FROM summaries s JOIN threads t ON t.id = s.thread \
             WHERE t.private_to IS NULL AND s.id = (SELECT max(id) FROM summaries \
               WHERE thread = t.id) AND instr(s.body, ?1) > 0",
        )?;
        for page in &pages {
            let label = format!("{} (rev {}, {}, owner {})", page.slug, page.rev, date(page.updated), page.owner);

            // Cited threads that have moved on since the page was last written.
            let threads = source_threads(&page.sources);
            cited_threads.extend(&threads);
            cited_threads.extend(hash_numbers(&page.body));
            let mut moved = Vec::new();
            for thread in &threads {
                if *thread == skip || self.thread_private_to(*thread).ok().flatten().is_some() {
                    continue;
                }
                let (count, last): (i64, Option<i64>) =
                    posts_since.query_row(params![thread, page.updated], |row| {
                        Ok((row.get(0)?, row.get(1)?))
                    })?;
                if let Some(last) = last {
                    moved.push(format!("#{thread}: {count} posts, last {}", date(last)));
                }
            }
            if !moved.is_empty() {
                behind.push(format!("- {label}: {}", moved.join("; ")));
            }

            // Citations of posts that a later post corrected.
            let cited: BTreeSet<i64> = post_refs(&page.body)
                .into_iter()
                .chain(page.sources.iter().flat_map(|source| post_refs(source)))
                .collect();
            for post in cited {
                let by: Option<Option<i64>> = superseded_by
                    .query_row([post], |row| row.get(0))
                    .optional()?;
                if let Some(Some(by)) = by {
                    superseded.push(format!("- {}: post {post} is superseded by post {by}", page.slug));
                }
            }

            if page.sources.is_empty() && !has_citation(&page.body) {
                uncited.push(format!("- {label}"));
            }

            let linked_from_page = pages
                .iter()
                .any(|other| other.slug != page.slug && mentions_slug(&other.body, &page.slug));
            let linked = linked_from_page
                || PROMPT_PAGES.contains(&page.slug.as_str())
                || linking_posts
                    .query_map([&page.slug], |row| row.get::<_, String>(0))?
                    .filter_map(Result::ok)
                    .any(|text| mentions_slug(&text, &page.slug));
            if !linked {
                unlinked.push(format!("- {label}"));
            }

            if page.owner.trim().is_empty() {
                ownerless.push(format!("- {}: no owner", page.slug));
            } else if !roster.contains(&page.owner) {
                ownerless.push(format!("- {}: owner {} isn't on the roster", page.slug, page.owner));
            }
        }
        sections.push(Section {
            title: "Pages whose cited threads have posts since the page's last revision",
            items: behind,
        });
        sections.push(Section {
            title: "Pages citing superseded posts",
            items: superseded,
        });
        sections.push(Section {
            title: "Pages with no citations",
            items: uncited,
        });
        sections.push(Section {
            title: "Pages no other page, post or summary mentions",
            items: unlinked,
        });
        sections.push(Section {
            title: "Pages with no owner, or an owner who isn't on the roster",
            items: ownerless,
        });

        // Reference threads no page cites.
        let mut statement = self.conn.prepare(
            "SELECT t.id, t.title, t.owner FROM threads t JOIN thread_tags g ON g.thread = t.id \
             WHERE g.tag = 'reference' AND t.private_to IS NULL ORDER BY t.id",
        )?;
        let items = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .filter(|(id, _, _)| !cited_threads.contains(id))
            .map(|(id, title, owner)| format!("- #{id} {title} (owner {owner})"))
            .collect();
        sections.push(Section {
            title: "Threads tagged reference that no page cites",
            items,
        });

        let mut out = format!(
            "# Wiki lint {}\n\nMechanical checks only (agent-board lint); private sectors are \
             left out. {} pages checked.\n",
            date(now),
            pages.len()
        );
        for section in &sections {
            out.push_str(&format!("\n## {} ({})\n", section.title, section.items.len()));
            if section.items.is_empty() {
                out.push_str("none\n");
            }
            for item in &section.items {
                out.push_str(item);
                out.push('\n');
            }
        }
        Ok(out)
    }

    /// Posts `report` in the "Wiki lint" thread, opening it (owned by saya) if needed.
    /// Returns the thread and post ids.
    ///
    /// # Errors
    ///
    /// Fails on database errors or a report too long for a post.
    pub fn post_lint(&self, report: &str) -> Result<(i64, i64), AppError> {
        let post = NewPost {
            body: report.to_owned(),
            ..NewPost::default()
        };
        if let Some(thread) = self.lint_thread()? {
            return Ok((thread, self.add_post(AUTHOR, thread, &post)?));
        }
        let (thread, id) = self.create_thread(
            AUTHOR,
            &NewThread {
                title: THREAD_TITLE.to_owned(),
                tags: vec!["wiki".to_owned(), "lint".to_owned()],
                summary: "The weekly wiki lint: `agent-board lint` posts a mechanical report here \
                          on Sundays, then saya's review turn works through it (board #36)."
                    .to_owned(),
                due: None,
                waiting_on: None,
                waiting_ref: None,
                post: Some(post),
            },
            false,
        )?;
        self.update_thread(
            AUTHOR,
            thread,
            &ThreadUpdate {
                owner: Some(OWNER.to_owned()),
                ..ThreadUpdate::default()
            },
        )?;
        Ok((thread, id.unwrap_or_default()))
    }
}

/// Today's report, posted. Returns the thread and post ids.
///
/// # Errors
///
/// As `lint_report` and `post_lint`.
pub fn run(board: &Board, roster: &[String], post: bool) -> Result<(String, Option<(i64, i64)>), AppError> {
    let report = board.lint_report(roster, now())?;
    let posted = if post { Some(board.post_lint(&report)?) } else { None };
    Ok((report, posted))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::NewTodo;
    use crate::pages::PageWrite;

    const NOW: i64 = 1_800_000_000;

    fn roster() -> Vec<String> {
        vec!["saya".into(), "tsugumi-lab".into(), "baughn".into()]
    }

    fn thread(board: &Board, title: &str, tags: &[&str]) -> i64 {
        board
            .create_thread(
                "tsugumi-lab",
                &NewThread {
                    title: title.into(),
                    tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
                    summary: "state".into(),
                    due: None,
                    waiting_on: None,
                    waiting_ref: None,
                    post: None,
                },
                false,
            )
            .unwrap()
            .0
    }

    fn post(board: &Board, thread: i64, body: &str) -> i64 {
        board
            .add_post(
                "tsugumi-lab",
                thread,
                &NewPost {
                    body: body.into(),
                    ..NewPost::default()
                },
            )
            .unwrap()
    }

    fn page(board: &Board, slug: &str, body: &str, sources: &[&str], owner: &str) {
        board
            .write_page(
                owner,
                &PageWrite {
                    slug: slug.into(),
                    body: body.into(),
                    title: Some(format!("Title of {slug}")),
                    note: "test".into(),
                    sources: sources.iter().map(|s| (*s).to_owned()).collect(),
                    ..PageWrite::default()
                },
                false,
            )
            .unwrap();
    }

    fn at(board: &Board, sql: &str, time: i64) {
        board.conn.execute(sql, [time]).unwrap();
    }

    /// The items under the section whose title starts with `title`.
    fn section<'a>(report: &'a str, title: &str) -> Vec<&'a str> {
        let start = report
            .find(&format!("## {title}"))
            .unwrap_or_else(|| panic!("no section {title:?} in\n{report}"));
        report[start..]
            .lines()
            .skip(1)
            .take_while(|line| !line.starts_with("## "))
            .filter(|line| !line.is_empty() && *line != "none")
            .collect()
    }

    #[test]
    fn parses_citations() {
        let sources = ["#43", "board #7", "thread #9", "#43a", "1565341055434625156", "ea@6041cd2f"];
        let sources: Vec<String> = sources.iter().map(|s| (*s).to_owned()).collect();
        assert_eq!(source_threads(&sources), BTreeSet::from([7, 9, 43]));
        assert_eq!(hash_numbers("see #12, PR#973 and (#5)"), BTreeSet::from([12, 5]));
        assert_eq!(post_refs("Post 262; post #3; repost 9; posts 1"), BTreeSet::from([262, 3]));
        assert!(has_citation("from msg 1564094295739207781"));
        assert!(has_citation("fixed in ea@6041cd2f."));
        assert!(has_citation("see 1564094295739207781"));
        assert!(has_citation("board #35"));
        assert!(!has_citation("no sources, an email a@b and 2026-10-30"));
        assert!(mentions_slug("see mod/ea.", "mod/ea"));
        assert!(mentions_slug("`mod/ea`", "mod/ea"));
        assert!(!mentions_slug("see mod/ea-shaft", "mod/ea"));
        assert!(!mentions_slug("see wmod/ea", "mod/ea"));
    }

    #[test]
    fn reports_each_check() {
        let board = Board::open_in_memory().unwrap();
        let old = thread(&board, "Old work", &[]);
        let busy = thread(&board, "Busy work", &[]);
        let fresh = thread(&board, "Fresh work", &[]);
        let reference = thread(&board, "Reference notes", &["reference"]);
        let cited_reference = thread(&board, "Cited reference", &["reference"]);
        let secret = board
            .create_thread(
                "tsugumi-sec",
                &NewThread {
                    title: "Secret".into(),
                    tags: vec!["reference".into()],
                    summary: String::new(),
                    due: None,
                    waiting_on: None,
                    waiting_ref: None,
                    post: None,
                },
                true,
            )
            .unwrap()
            .0;
        let wrong = post(&board, busy, "measured 5");
        let right = post(&board, busy, "measured 6");
        board
            .add_post(
                "tsugumi-lab",
                busy,
                &NewPost {
                    body: "correction".into(),
                    supersedes: vec![wrong],
                    ..NewPost::default()
                },
            )
            .unwrap();
        page(&board, "mod/ea", &format!("Cites post {wrong} and post {right}; see mod/ea-shaft"), &["#2"], "tsugumi-lab");
        page(&board, "mod/ea-shaft", &format!("Uses #{cited_reference}. Mentioned on mod/ea."), &[], "tsugumi-lab");
        page(&board, "mod/lonely", "No citations at all.", &[], "argon");
        page(&board, "wiki/conventions", "Rules, from #36.", &[], "saya");
        board
            .write_page(
                "tsugumi-sec",
                &PageWrite {
                    slug: "sec/private".into(),
                    body: "private".into(),
                    title: Some("Private".into()),
                    note: "test".into(),
                    ..PageWrite::default()
                },
                true,
            )
            .unwrap();
        post(&board, fresh, "only mod/lonelier here");
        let old_todo = board
            .create_todo(
                "tsugumi-lab",
                "saya",
                &NewTodo {
                    title: "Check the thing".into(),
                    thread: Some(old),
                    ..NewTodo::default()
                },
                false,
            )
            .unwrap()
            .id;
        board
            .create_todo(
                "tsugumi-lab",
                "saya",
                &NewTodo {
                    title: "New task".into(),
                    ..NewTodo::default()
                },
                false,
            )
            .unwrap();

        // Ages: `old` and its todo are 20 days old; `busy` got posts 10 days after its
        // summary, after the pages were written; the rest is recent.
        let day = DAY;
        at(&board, "UPDATE threads SET created = ?1, updated = ?1", NOW - 30 * day);
        at(&board, "UPDATE summaries SET created = ?1", NOW - 30 * day);
        at(&board, "UPDATE threads SET updated = ?1 WHERE id != 1", NOW - day);
        at(&board, "UPDATE threads SET updated = ?1 WHERE id = 1", NOW - 20 * day);
        at(&board, "UPDATE page_revisions SET created = ?1", NOW - 25 * day);
        at(&board, "UPDATE posts SET created = ?1", NOW - 20 * day);
        at(&board, &format!("UPDATE posts SET created = ?1 WHERE thread != {busy}"), NOW - 29 * day);
        at(&board, &format!("UPDATE todos SET created = ?1 WHERE id = {old_todo}"), NOW - 20 * day);
        at(&board, &format!("UPDATE todos SET created = ?1 WHERE id != {old_todo}"), NOW - day);
        assert_eq!((old, busy), (1, 2));

        let report = board.lint_report(&roster(), NOW).unwrap();
        assert!(report.starts_with("# Wiki lint 2027-01-15\n"), "{report}");
        assert!(!report.contains("Secret") && !report.contains("sec/private"), "{report}");
        assert_eq!(
            section(&report, "Open threads untouched"),
            ["- #1 Old work (owner tsugumi-lab, last change 2026-12-26)"]
        );
        assert_eq!(
            section(&report, "Summaries more than"),
            ["- #2 Busy work (owner tsugumi-lab): summary 2026-12-16, last post 2026-12-26"]
        );
        assert_eq!(
            section(&report, "Todos open"),
            ["- saya (1)", "  - todo #1 Check the thing (since 2026-12-26, thread #1)"]
        );
        assert_eq!(
            section(&report, "Pages whose cited threads"),
            ["- mod/ea (rev 1, 2026-12-21, owner tsugumi-lab): #2: 3 posts, last 2026-12-26"]
        );
        assert_eq!(
            section(&report, "Pages citing superseded"),
            [format!("- mod/ea: post {wrong} is superseded by post {}", right + 1).as_str()]
        );
        assert_eq!(
            section(&report, "Pages with no citations"),
            ["- mod/lonely (rev 1, 2026-12-21, owner argon)"]
        );
        // mod/ea and mod/ea-shaft name each other; wiki/conventions is in every prompt;
        // "mod/lonelier" isn't "mod/lonely".
        assert_eq!(
            section(&report, "Pages no other page"),
            ["- mod/lonely (rev 1, 2026-12-21, owner argon)"]
        );
        assert_eq!(
            section(&report, "Pages with no owner"),
            ["- mod/lonely: owner argon isn't on the roster"]
        );
        assert_eq!(
            section(&report, "Threads tagged reference"),
            [format!("- #{reference} Reference notes (owner tsugumi-lab)").as_str()]
        );
        let _ = secret;
    }

    #[test]
    fn posts_into_one_thread_owned_by_saya() {
        let board = Board::open_in_memory().unwrap();
        let empty = board.lint_report(&roster(), NOW).unwrap();
        assert_eq!(empty.matches("\nnone\n").count(), 9, "{empty}");

        let (thread, first) = board.post_lint(&empty).unwrap();
        let (again, second) = board.post_lint("second").unwrap();
        assert_eq!(thread, again);
        assert!(second > first);
        let view = board.read_thread(thread, None, None).unwrap();
        assert_eq!(view.thread.owner, OWNER);
        assert_eq!(view.thread.title, THREAD_TITLE);
        assert!(view.posts.iter().all(|post| post.author == AUTHOR));
        // The lint thread doesn't report on itself, however stale.
        at(&board, "UPDATE threads SET updated = ?1", NOW - 30 * DAY);
        at(&board, "UPDATE summaries SET created = ?1", NOW - 30 * DAY);
        let report = board.lint_report(&roster(), NOW).unwrap();
        assert!(!report.contains(&format!("#{thread} ")), "{report}");
    }
}
