//! The searchable copy of the Discord channel, fed by the agent-board-discord poller.
//!
//! Messages arrive cleaned up (mentions resolved to names, text attachments inlined) and
//! are upserted: a later copy with a different edit time or content replaces the row and
//! its search entry. Discord stays the source; this is a copy.

use chrono::NaiveDate;
use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use crate::db::{Board, MAX_ATTACHMENT, MAX_BODY};
use crate::error::AppError;

/// The identity the poller posts as; nobody else may write to the archive.
pub const INGEST_IDENTITY: &str = "discord";

const AUTHOR_KINDS: &[&str] = &["human", "agent", "webhook", "approval", "other"];

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IncomingAttachment {
    pub name: String,
    pub size: i64,
    /// The text of a text attachment up to 1 MiB; none for images and binaries.
    pub text: Option<String>,
}

/// A message as the poller sends it.
#[derive(Clone, Debug, Deserialize)]
pub struct IncomingMessage {
    pub id: i64,
    /// The channel, or a thread's parent channel.
    pub channel: i64,
    /// The Discord thread, for messages in one.
    pub thread: Option<i64>,
    pub author: String,
    /// human, agent, webhook, approval (an agent's permission request) or other.
    pub author_kind: String,
    pub created: i64,
    pub edited: Option<i64>,
    pub reply_to: Option<i64>,
    pub content: String,
    #[serde(default)]
    pub attachments: Vec<IncomingAttachment>,
    pub url: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct AttachmentSummary {
    pub index: usize,
    pub name: String,
    pub size: i64,
    pub has_text: bool,
}

/// An archived message; attachment texts are fetched separately.
#[derive(Clone, Debug, Serialize)]
pub struct ArchivedMessage {
    pub id: i64,
    pub channel: i64,
    pub thread: Option<i64>,
    pub author: String,
    pub author_kind: String,
    pub created: i64,
    pub edited: Option<i64>,
    pub reply_to: Option<i64>,
    pub content: String,
    pub attachments: Vec<AttachmentSummary>,
    pub url: String,
}

/// A message with its neighbours in the same channel or thread, oldest first.
#[derive(Clone, Debug, Serialize)]
pub struct MessageContext {
    pub focus: i64,
    pub messages: Vec<ArchivedMessage>,
}

#[derive(Clone, Debug, Serialize)]
pub struct IngestResult {
    pub inserted: usize,
    pub updated: usize,
    pub unchanged: usize,
}

const COLUMNS: &str =
    "id, channel, thread, author, author_kind, created, edited, reply_to, content, attachments, url";

fn message_from_row(row: &Row) -> rusqlite::Result<ArchivedMessage> {
    let attachments: String = row.get(9)?;
    let attachments: Vec<IncomingAttachment> =
        serde_json::from_str(&attachments).unwrap_or_default();
    Ok(ArchivedMessage {
        id: row.get(0)?,
        channel: row.get(1)?,
        thread: row.get(2)?,
        author: row.get(3)?,
        author_kind: row.get(4)?,
        created: row.get(5)?,
        edited: row.get(6)?,
        reply_to: row.get(7)?,
        content: row.get(8)?,
        attachments: attachments
            .into_iter()
            .enumerate()
            .map(|(index, file)| AttachmentSummary {
                index,
                name: file.name,
                size: file.size,
                has_text: file.text.is_some(),
            })
            .collect(),
        url: row.get(10)?,
    })
}

fn validate(message: &IncomingMessage) -> Result<(), AppError> {
    let bad = |what: String| {
        Err(AppError::BadRequest(format!(
            "message {}: {what}",
            message.id
        )))
    };
    if !AUTHOR_KINDS.contains(&message.author_kind.as_str()) {
        return bad(format!("author_kind {:?}", message.author_kind));
    }
    if message.author.is_empty() || message.author.len() > 100 {
        return bad("author must be 1-100 bytes".into());
    }
    if message.content.len() > MAX_BODY {
        return bad(format!("content is longer than {MAX_BODY} bytes"));
    }
    for file in &message.attachments {
        if file
            .text
            .as_ref()
            .is_some_and(|text| text.len() > MAX_ATTACHMENT)
        {
            return bad(format!(
                "attachment {:?} is longer than {MAX_ATTACHMENT} bytes",
                file.name
            ));
        }
    }
    Ok(())
}

impl Board {
    /// Stores or refreshes archived messages in one transaction.
    ///
    /// # Errors
    ///
    /// Returns `BadRequest` if any message is invalid; nothing is stored then.
    pub fn ingest_discord(&self, messages: &[IncomingMessage]) -> Result<IngestResult, AppError> {
        messages.iter().try_for_each(validate)?;
        let mut result = IngestResult {
            inserted: 0,
            updated: 0,
            unchanged: 0,
        };
        // The mutex around the board makes this connection ours alone.
        let tx = self.conn.unchecked_transaction()?;
        for message in messages {
            let existing: Option<(i64, Option<i64>, String)> = tx
                .query_row(
                    "SELECT search_row, edited, content FROM discord WHERE id = ?1",
                    [message.id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()?;
            if let Some((_, edited, content)) = &existing {
                if *edited == message.edited && *content == message.content {
                    result.unchanged += 1;
                    continue;
                }
            }
            let titles: Vec<&str> = message
                .attachments
                .iter()
                .map(|f| f.name.as_str())
                .collect();
            let mut body = message.content.clone();
            for text in message.attachments.iter().filter_map(|f| f.text.as_deref()) {
                body.push_str("\n\n");
                body.push_str(text);
            }
            if let Some((search_row, _, _)) = existing {
                tx.execute("DELETE FROM search WHERE rowid = ?1", [search_row])?;
            }
            tx.execute(
                "INSERT INTO search (kind, ref, thread, author, created, title, body) \
                 VALUES ('discord', ?1, NULL, ?2, ?3, ?4, ?5)",
                params![
                    message.id,
                    message.author,
                    message.created,
                    titles.join(" "),
                    body
                ],
            )?;
            let search_row = tx.last_insert_rowid();
            tx.execute(
                "INSERT INTO discord (id, channel, thread, author, author_kind, created, edited, \
                     reply_to, content, attachments, url, search_row) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12) \
                 ON CONFLICT (id) DO UPDATE SET author = excluded.author, \
                     author_kind = excluded.author_kind, edited = excluded.edited, \
                     content = excluded.content, attachments = excluded.attachments, \
                     url = excluded.url, search_row = excluded.search_row",
                params![
                    message.id,
                    message.channel,
                    message.thread,
                    message.author,
                    message.author_kind,
                    message.created,
                    message.edited,
                    message.reply_to,
                    message.content,
                    serde_json::to_string(&message.attachments).map_err(anyhow::Error::from)?,
                    message.url,
                    search_row,
                ],
            )?;
            if existing.is_some() {
                result.updated += 1;
            } else {
                result.inserted += 1;
            }
        }
        tx.commit()?;
        Ok(result)
    }

    /// The newest archived message id in a channel (or one of its threads).
    ///
    /// # Errors
    ///
    /// Only on database errors.
    pub fn discord_cursor(
        &self,
        channel: i64,
        thread: Option<i64>,
    ) -> Result<Option<i64>, AppError> {
        Ok(self.conn.query_row(
            "SELECT max(id) FROM discord WHERE channel = ?1 AND thread IS ?2",
            params![channel, thread],
            |row| row.get(0),
        )?)
    }

    /// A message with up to `context` neighbours on each side, in its channel or thread.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` for an unknown message.
    pub fn discord_context(&self, id: i64, context: usize) -> Result<MessageContext, AppError> {
        let focus = self
            .conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM discord WHERE id = ?1"),
                [id],
                message_from_row,
            )
            .optional()?
            .ok_or_else(|| AppError::NotFound(format!("discord message {id}")))?;
        let limit = i64::try_from(context.min(50)).unwrap_or(50);
        let (channel, thread) = (focus.channel, focus.thread);
        let neighbours = |condition: &str, order: &str| -> Result<Vec<ArchivedMessage>, AppError> {
            let sql = format!(
                "SELECT {COLUMNS} FROM discord WHERE channel = ?1 AND thread IS ?2 AND {condition} \
                 ORDER BY id {order} LIMIT ?4"
            );
            let mut statement = self.conn.prepare_cached(&sql)?;
            let rows =
                statement.query_map(params![channel, thread, id, limit], message_from_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        };
        let mut messages = neighbours("id < ?3", "DESC")?;
        messages.reverse();
        messages.push(focus);
        messages.extend(neighbours("id > ?3", "ASC")?);
        Ok(MessageContext {
            focus: id,
            messages,
        })
    }

    /// The text of one attachment of an archived message.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` if there is no such message, attachment or text.
    pub fn discord_attachment(
        &self,
        id: i64,
        index: usize,
    ) -> Result<IncomingAttachment, AppError> {
        let attachments: String = self
            .conn
            .query_row(
                "SELECT attachments FROM discord WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| AppError::NotFound(format!("discord message {id}")))?;
        let attachments: Vec<IncomingAttachment> =
            serde_json::from_str(&attachments).map_err(anyhow::Error::from)?;
        attachments
            .into_iter()
            .nth(index)
            .filter(|file| file.text.is_some())
            .ok_or_else(|| AppError::NotFound(format!("text attachment {index} of {id}")))
    }

    /// All archived messages from one UTC day, oldest first.
    ///
    /// # Errors
    ///
    /// Returns `BadRequest` for a malformed date.
    pub fn discord_day(&self, day: &str) -> Result<Vec<ArchivedMessage>, AppError> {
        let start = NaiveDate::parse_from_str(day, "%Y-%m-%d")
            .map_err(|_| AppError::BadRequest(format!("day {day:?}: use YYYY-MM-DD")))?
            .and_hms_opt(0, 0, 0)
            .map(|at| at.and_utc().timestamp())
            .unwrap_or_default();
        let mut statement = self.conn.prepare_cached(&format!(
            "SELECT {COLUMNS} FROM discord WHERE created >= ?1 AND created < ?2 ORDER BY id"
        ))?;
        let rows = statement.query_map([start, start + 86_400], message_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::SearchQuery;

    fn message(id: i64, thread: Option<i64>, content: &str) -> IncomingMessage {
        IncomingMessage {
            id,
            channel: 100,
            thread,
            author: "baughn".into(),
            author_kind: "human".into(),
            created: 1_759_400_000 + id,
            edited: None,
            reply_to: None,
            content: content.into(),
            attachments: Vec::new(),
            url: format!("https://discord.com/channels/1/100/{id}"),
        }
    }

    #[test]
    fn upserts_and_searches() {
        let board = Board::open_in_memory().unwrap();
        let mut plan = message(1, None, "Here is the plan");
        plan.attachments.push(IncomingAttachment {
            name: "plan.md".into(),
            size: 20,
            text: Some("use restic for offsite backups".into()),
        });
        let batch = [
            plan,
            message(2, None, "working..."),
            message(3, Some(50), "in a thread"),
        ];
        let result = board.ingest_discord(&batch).unwrap();
        assert_eq!(
            (result.inserted, result.updated, result.unchanged),
            (3, 0, 0)
        );
        assert_eq!(board.ingest_discord(&batch[..1]).unwrap().unchanged, 1);

        let mut edited = message(2, None, "done: shipped the board");
        edited.edited = Some(1_759_400_100);
        assert_eq!(board.ingest_discord(&[edited]).unwrap().updated, 1);

        let search = |q: &str| {
            board
                .search(&SearchQuery {
                    q: q.into(),
                    kind: Some("discord".into()),
                    ..Default::default()
                })
                .unwrap()
        };
        assert_eq!(search("restic")[0].id, 1);
        assert_eq!(search("shipped")[0].id, 2);
        assert!(search("working").is_empty(), "the old text left the index");

        assert_eq!(board.discord_cursor(100, None).unwrap(), Some(2));
        assert_eq!(board.discord_cursor(100, Some(50)).unwrap(), Some(3));
        assert_eq!(board.discord_cursor(101, None).unwrap(), None);

        let context = board.discord_context(1, 5).unwrap();
        let ids: Vec<i64> = context.messages.iter().map(|m| m.id).collect();
        assert_eq!(ids, vec![1, 2], "the thread's message is elsewhere");
        assert!(context.messages[0].attachments[0].has_text);
        assert_eq!(board.discord_attachment(1, 0).unwrap().name, "plan.md");
        assert!(board.discord_attachment(1, 1).is_err());
    }

    #[test]
    fn rejects_unknown_kinds() {
        let board = Board::open_in_memory().unwrap();
        let mut bad = message(1, None, "x");
        bad.author_kind = "robot".into();
        assert!(matches!(
            board.ingest_discord(&[bad]),
            Err(AppError::BadRequest(_))
        ));
    }
}
