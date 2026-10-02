//! Status cards for the dashboard: small structured reports that agents' bridges and
//! collectors (the lab timer) replace in place, keyed by (agent, key). A card older than
//! its ttl shows as stale, so a dead writer is visible instead of looking idle.

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::db::{now, Ask, Board, Thread};
use crate::discord::INGEST_IDENTITY;
use crate::error::AppError;

const MAX_TITLE: usize = 100;
const MAX_STATE: usize = 80;
const MAX_LINES: usize = 50;
const MAX_LINE: usize = 400;
const MAX_LINK: usize = 500;
const MIN_TTL: i64 = 60;
const MAX_TTL: i64 = 86_400;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Ok,
    Info,
    Warn,
    Alert,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StatusLine {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<Level>,
}

/// A card as its writer sends it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NewStatus {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<Level>,
    #[serde(default)]
    pub lines: Vec<StatusLine>,
    /// Seconds after which the card counts as stale.
    pub ttl: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct StatusCard {
    pub agent: String,
    pub key: String,
    pub updated: i64,
    pub stale: bool,
    #[serde(flatten)]
    pub body: NewStatus,
}

/// Everything the dashboard shows.
#[derive(Clone, Debug, Serialize)]
pub struct Dashboard {
    pub now: i64,
    pub cards: Vec<StatusCard>,
    pub asks: Vec<Ask>,
    pub waiting: Vec<Thread>,
    pub due: Vec<Thread>,
}

fn bad(message: impl Into<String>) -> AppError {
    AppError::BadRequest(message.into())
}

fn check_key(key: &str) -> Result<(), AppError> {
    let valid = !key.is_empty()
        && key.len() <= 32
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if valid {
        Ok(())
    } else {
        Err(bad(format!("key {key:?}: use 1-32 of a-z, 0-9 and -")))
    }
}

fn check(status: &NewStatus) -> Result<(), AppError> {
    if status.title.trim().is_empty() || status.title.len() > MAX_TITLE {
        return Err(bad(format!("title: 1-{MAX_TITLE} bytes")));
    }
    if status.state.as_ref().is_some_and(|state| state.len() > MAX_STATE) {
        return Err(bad(format!("state: at most {MAX_STATE} bytes")));
    }
    if status.lines.len() > MAX_LINES {
        return Err(bad(format!("at most {MAX_LINES} lines")));
    }
    for line in &status.lines {
        if line.text.len() > MAX_LINE {
            return Err(bad(format!("line: at most {MAX_LINE} bytes")));
        }
        if line.link.as_ref().is_some_and(|link| link.len() > MAX_LINK) {
            return Err(bad(format!("link: at most {MAX_LINK} bytes")));
        }
    }
    if !(MIN_TTL..=MAX_TTL).contains(&status.ttl) {
        return Err(bad(format!("ttl: {MIN_TTL}-{MAX_TTL} seconds")));
    }
    Ok(())
}

impl Board {
    /// Replaces `agent`'s card `key`.
    ///
    /// # Errors
    ///
    /// Bad keys or oversized cards; the archive identity may not write cards.
    pub fn put_status(&self, agent: &str, key: &str, status: &NewStatus) -> Result<(), AppError> {
        if agent == INGEST_IDENTITY {
            return Err(AppError::Forbidden("the archive can't write status cards".into()));
        }
        check_key(key)?;
        check(status)?;
        let body = serde_json::to_string(status).map_err(anyhow::Error::from)?;
        self.conn.execute(
            "INSERT INTO status (agent, key, updated, ttl, body) VALUES (?1, ?2, ?3, ?4, ?5) \
             ON CONFLICT (agent, key) DO UPDATE SET \
               updated = excluded.updated, ttl = excluded.ttl, body = excluded.body",
            params![agent, key, now(), status.ttl, body],
        )?;
        Ok(())
    }

    /// Removes `agent`'s card `key`.
    ///
    /// # Errors
    ///
    /// Not found when there is no such card.
    pub fn delete_status(&self, agent: &str, key: &str) -> Result<(), AppError> {
        let removed = self.conn.execute(
            "DELETE FROM status WHERE agent = ?1 AND key = ?2",
            params![agent, key],
        )?;
        if removed == 0 {
            return Err(AppError::NotFound(format!("status card {key}")));
        }
        Ok(())
    }

    /// Every card, by agent and key.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn status_cards(&self) -> Result<Vec<StatusCard>, AppError> {
        let now = now();
        let mut statement = self
            .conn
            .prepare_cached("SELECT agent, key, updated, ttl, body FROM status ORDER BY agent, key")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        let mut cards = Vec::new();
        for row in rows {
            let (agent, key, updated, ttl, body) = row?;
            let body: NewStatus = serde_json::from_str(&body).map_err(anyhow::Error::from)?;
            cards.push(StatusCard {
                agent,
                key,
                updated,
                stale: now - updated > ttl,
                body,
            });
        }
        Ok(cards)
    }

    /// The dashboard: status cards, unanswered asks, waiting and due threads.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn dashboard(&self) -> Result<Dashboard, AppError> {
        let (waiting, due) = self.waiting_and_due()?;
        Ok(Dashboard {
            now: now(),
            cards: self.status_cards()?,
            asks: self.open_asks(None)?,
            waiting,
            due,
        })
    }
}
