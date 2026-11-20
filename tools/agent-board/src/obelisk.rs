//! The Obelisk of Changes feed (Baughn, msg 1573355320326230037; plan on board #73): one
//! document with what three agents are doing, for an in-game obelisk and a board page.
//!
//! Players read the obelisk, so this module is the egress boundary: everything it emits
//! passes [`sanitize`], whatever its source already did. It reads what the board has anyway
//! (bridge status cards, the Discord archive, threads, todos) plus a little of /proc.

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::db::{now, Board};
use crate::error::AppError;
use crate::status::StatusCard;

/// Recent headlines per agent.
const SAID: usize = 4;
/// Chatter lines per agent.
const CHATTER: usize = 20;
/// Longest string in the document, in characters.
const MAX_TEXT: usize = 120;
/// The card key the merge queue writes its card under (any agent).
const QUEUE_KEY: &str = "queue";
const WEEK: i64 = 7 * 86_400;

/// Who stands on the obelisk, in face order (service config).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub agents: Vec<Agent>,
    /// Where to keep the document for the mod (rewritten every [`WRITE_EVERY`], with a fresh
    /// `t` even when nothing changed, so readers can tell a quiet board from a dead one).
    #[serde(default)]
    pub file: Option<std::path::PathBuf>,
}

/// How often the file is rewritten.
pub const WRITE_EVERY: std::time::Duration = std::time::Duration::from_secs(10);

/// Writes `doc` to `path` atomically (a temporary file in the same directory, then
/// rename), readable by the directory's group.
///
/// # Errors
///
/// I/O errors.
pub fn write_file(doc: &Document, path: &std::path::Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let json = serde_json::to_vec(doc)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o640))?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Rewrites the file forever; a failure is logged and retried next round.
pub async fn write_loop(state: crate::api::AppState, path: std::path::PathBuf) {
    let mut ticker = tokio::time::interval(WRITE_EVERY);
    loop {
        ticker.tick().await;
        let result = match state.obelisk().await {
            Ok(doc) => {
                let path = path.clone();
                tokio::task::spawn_blocking(move || write_file(&doc, &path))
                    .await
                    .map_err(anyhow::Error::from)
                    .and_then(|written| written)
            }
            Err(error) => Err(anyhow::anyhow!("{error:?}")),
        };
        if let Err(error) = result {
            tracing::warn!("obelisk file {}: {error:#}", path.display());
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Agent {
    pub id: String,
    pub name: String,
}

/// What a bridge adds to its status card for the obelisk (`obelisk` in the card body).
/// The bridge digests its tool calls from an allowlist; the board sanitizes them again.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CardExtra {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    /// When the topic was set (unix time).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic_since: Option<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chatter: Vec<ChatterIn>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatterIn {
    pub id: String,
    pub t: i64,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ok: Option<bool>,
}

/// Limits on [`CardExtra`], checked when a card is written.
pub const MAX_CHATTER: usize = 30;
pub const MAX_CHATTER_TEXT: usize = 200;
pub const MAX_TOPIC: usize = 200;
pub const MAX_ID: usize = 64;

/// The document. Every band has `changed`, the unix time its content last changed; readers
/// compute the glow from it and `t`.
#[derive(Clone, Debug, Serialize)]
pub struct Document {
    pub v: u32,
    pub t: i64,
    pub agents: Vec<Face>,
    pub forge: Forge,
}

#[derive(Clone, Debug, Serialize)]
pub struct Face {
    pub id: String,
    pub name: String,
    /// working, waiting, idle, paused or offline.
    pub state: &'static str,
    pub now: Now,
    pub said: Band,
    pub chatter: Band,
}

#[derive(Clone, Debug, Serialize)]
pub struct Now {
    pub changed: i64,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread: Option<i64>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Band {
    pub changed: i64,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Item {
    pub id: String,
    pub t: i64,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ok: Option<bool>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Forge {
    pub queue: Band,
    pub load: Load,
    pub ledger: Ledger,
    pub landed: Band,
}

/// tsugumi's load, each 0..1 (cpu is the 1-minute load average per CPU, capped).
#[derive(Clone, Debug, Default, Serialize)]
pub struct Load {
    pub changed: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mem: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub io: Option<f64>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Ledger {
    pub changed: i64,
    pub todos_closed_7d: i64,
    pub posts_7d: i64,
    pub pages_7d: i64,
}

/// Makes `text` safe to show players: no formatting codes, control, bidi or zero-width
/// characters, no Discord mentions, links or addresses, whitespace collapsed, at most
/// `max` characters. `None` when it looks like it carries a secret: drop it, don't redact.
#[must_use]
pub fn sanitize(text: &str, max: usize) -> Option<String> {
    let mut clean = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            // Minecraft's formatting codes: § and the code after it.
            '§' => {
                chars.next();
            }
            '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2069}'
            | '\u{FEFF}' => {}
            c if c.is_control() => clean.push(' '),
            c => clean.push(c),
        }
    }
    let mut words = Vec::new();
    for word in clean.split_whitespace() {
        if secret_like(word) {
            return None;
        }
        if word.contains("://") || word.starts_with("www.") {
            words.push("(link)");
        } else if word.starts_with("<@") || word.starts_with("<#") {
            continue;
        } else if address_like(word) {
            words.push("(address)");
        } else {
            words.push(word);
        }
    }
    let joined = words.join(" ");
    if joined.chars().count() <= max {
        return Some(joined);
    }
    let mut cut: String = joined.chars().take(max.saturating_sub(1)).collect();
    cut.push('…');
    Some(cut)
}

/// Token prefixes and long random-looking words (keys, hashes, tokens), also after `=` or
/// `:` as in `key=…`.
fn secret_like(word: &str) -> bool {
    word.split(['=', ':', '"', '\'']).any(secret_part)
}

fn secret_part(word: &str) -> bool {
    let word = word.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-');
    const PREFIXES: &[&str] = &[
        "ghp_",
        "gho_",
        "ghs_",
        "ghu_",
        "github_pat_",
        "sk-",
        "xox",
        "AKIA",
        "eyJ",
    ];
    if PREFIXES.iter().any(|prefix| word.starts_with(prefix)) && word.len() >= 12 {
        return true;
    }
    let tokenish = word
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+' | '/' | '='));
    let digits = word.chars().filter(char::is_ascii_digit).count();
    let letters = word.chars().filter(char::is_ascii_alphabetic).count();
    tokenish && word.len() >= 24 && digits >= 3 && letters >= 3
}

/// An IPv4 address (with or without port), or something with several colons and hex digits
/// (IPv6).
fn address_like(word: &str) -> bool {
    let word = word.trim_matches(|c: char| matches!(c, '(' | ')' | ',' | '.' | '[' | ']' | '`'));
    let host = word.split(':').next().unwrap_or(word);
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() == 4
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 3 && p.bytes().all(|b| b.is_ascii_digit()))
    {
        return true;
    }
    word.matches(':').count() >= 3
        && word
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == ':' || c == '/')
}

/// A bridge's post as one headline: its first line, without the leading emoji and bold.
fn headline(content: &str) -> Option<String> {
    let first = content.lines().find(|line| !line.trim().is_empty())?;
    let trimmed = first.trim_start_matches(|c: char| !c.is_alphanumeric() && c != '*' && c != '`');
    let text = trimmed.replace("**", "");
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// Bridge status messages ("⚙ name · working for …") and approval requests aren't headlines.
fn is_noise(content: &str) -> bool {
    let first = content.trim_start();
    first.starts_with('\u{1F510}')
        || (first.contains(" · working for ")
            && first.starts_with(['⚙', '✓', '✗', '⏹', '\u{1F4A4}']))
}

fn state_of(card: Option<&StatusCard>) -> &'static str {
    let Some(card) = card else { return "offline" };
    if card.stale {
        return "offline";
    }
    let state = card.body.state.as_deref().unwrap_or("");
    if state.starts_with("paused") || state.starts_with("circuit") {
        "paused"
    } else if state.starts_with("working") {
        if matches!(card.body.level, Some(crate::status::Level::Alert)) {
            "waiting"
        } else {
            "working"
        }
    } else {
        "idle"
    }
}

fn band(mut items: Vec<Item>) -> Band {
    items.sort_by_key(|item| std::cmp::Reverse(item.t));
    Band {
        changed: items.first().map_or(0, |item| item.t),
        items,
    }
}

/// /proc numbers; `None` where the file is missing (another OS, a sandbox).
fn load(t: i64) -> Load {
    let read = |path: &str| std::fs::read_to_string(path).ok();
    let cpus = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    #[allow(clippy::cast_precision_loss)]
    let cpu = read("/proc/loadavg")
        .and_then(|text| text.split_whitespace().next()?.parse::<f64>().ok())
        .map(|load| (load / cpus as f64).min(1.0));
    let mem = read("/proc/meminfo").and_then(|text| {
        let field = |name: &str| {
            text.lines()
                .find(|line| line.starts_with(name))?
                .split_whitespace()
                .nth(1)?
                .parse::<f64>()
                .ok()
        };
        let total = field("MemTotal:")?;
        let available = field("MemAvailable:")?;
        (total > 0.0).then(|| 1.0 - available / total)
    });
    let io = read("/proc/pressure/io").and_then(|text| {
        let some = text.lines().find(|line| line.starts_with("some"))?;
        let avg = some
            .split_whitespace()
            .find_map(|field| field.strip_prefix("avg10="))?;
        avg.parse::<f64>()
            .ok()
            .map(|percent| (percent / 100.0).min(1.0))
    });
    let round = |value: f64| (value * 1000.0).round() / 1000.0;
    Load {
        changed: t,
        cpu: cpu.map(round),
        mem: mem.map(round),
        io: io.map(round),
    }
}

impl Board {
    /// The obelisk document for `config`'s agents.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub fn obelisk(&self, config: &Config) -> Result<Document, AppError> {
        let t = now();
        let cards = self.status_cards()?;
        let mut agents = Vec::with_capacity(config.agents.len());
        for agent in &config.agents {
            let card = cards
                .iter()
                .find(|card| card.agent == agent.id && card.key == "bridge");
            agents.push(Face {
                id: agent.id.clone(),
                name: agent.name.clone(),
                state: state_of(card),
                now: self.obelisk_now(&agent.id, card)?,
                said: self.obelisk_said(&agent.id)?,
                chatter: chatter(card),
            });
        }
        let queue = cards
            .iter()
            .find(|card| card.key == QUEUE_KEY && !card.stale)
            .map_or_else(Band::default, |card| {
                let items = card
                    .body
                    .lines
                    .iter()
                    .enumerate()
                    .filter_map(|(index, line)| {
                        Some(Item {
                            id: format!("{}-{index}", card.agent),
                            t: card.updated,
                            text: sanitize(&line.text, MAX_TEXT)?,
                            ok: None,
                        })
                    })
                    .collect();
                Band {
                    changed: card.updated,
                    items,
                }
            });
        Ok(Document {
            v: 1,
            t,
            agents,
            forge: Forge {
                queue,
                load: load(t),
                ledger: self.obelisk_ledger(t)?,
                landed: Band::default(),
            },
        })
    }

    /// The topic the bridge gave, else what its card says it is doing, else the agent's most
    /// recently active open thread.
    fn obelisk_now(&self, agent: &str, card: Option<&StatusCard>) -> Result<Now, AppError> {
        if let Some(card) = card.filter(|card| !card.stale) {
            if let Some(extra) = &card.body.obelisk {
                if let Some(text) = extra
                    .topic
                    .as_deref()
                    .and_then(|topic| sanitize(topic, MAX_TEXT))
                {
                    if !text.is_empty() {
                        return Ok(Now {
                            changed: extra.topic_since.unwrap_or(card.updated),
                            text,
                            thread: None,
                        });
                    }
                }
            }
        }
        let thread: Option<(i64, String, i64)> = self
            .conn
            .prepare_cached(
                "SELECT id, title, updated FROM threads WHERE owner = ?1 AND status = 'open' \
                 AND private_to IS NULL ORDER BY updated DESC LIMIT 1",
            )?
            .query_row(params![agent], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .map(Some)
            .or_else(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })?;
        Ok(match thread {
            Some((id, title, updated)) => Now {
                changed: updated,
                text: sanitize(&title, MAX_TEXT).unwrap_or_default(),
                thread: Some(id),
            },
            None => Now {
                changed: 0,
                text: String::new(),
                thread: None,
            },
        })
    }

    /// The agent's last few Discord headlines.
    fn obelisk_said(&self, agent: &str) -> Result<Band, AppError> {
        let mut statement = self.conn.prepare_cached(
            "SELECT id, created, content FROM discord WHERE author = ?1 AND author_kind = 'agent' \
             ORDER BY id DESC LIMIT 20",
        )?;
        let rows = statement.query_map(params![agent], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut items = Vec::new();
        for row in rows {
            let (id, created, content) = row?;
            if is_noise(&content) {
                continue;
            }
            let Some(text) = headline(&content).and_then(|line| sanitize(&line, MAX_TEXT)) else {
                continue;
            };
            if text.is_empty() {
                continue;
            }
            items.push(Item {
                id: id.to_string(),
                t: created,
                text,
                ok: None,
            });
            if items.len() == SAID {
                break;
            }
        }
        Ok(band(items))
    }

    fn obelisk_ledger(&self, t: i64) -> Result<Ledger, AppError> {
        let since = t - WEEK;
        let (todos, changed): (i64, Option<i64>) = self.conn.query_row(
            "SELECT count(*), max(closed) FROM todos WHERE status = 'done' AND closed >= ?1",
            params![since],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let posts: i64 = self.conn.query_row(
            "SELECT count(*) FROM posts p JOIN threads t ON t.id = p.thread \
             WHERE p.created >= ?1 AND t.private_to IS NULL",
            params![since],
            |row| row.get(0),
        )?;
        let pages: i64 = self.conn.query_row(
            "SELECT count(*) FROM page_revisions WHERE created >= ?1",
            params![since],
            |row| row.get(0),
        )?;
        Ok(Ledger {
            changed: changed.unwrap_or(0),
            todos_closed_7d: todos,
            posts_7d: posts,
            pages_7d: pages,
        })
    }
}

fn chatter(card: Option<&StatusCard>) -> Band {
    let Some(extra) = card.and_then(|card| card.body.obelisk.as_ref()) else {
        return Band::default();
    };
    let items = extra
        .chatter
        .iter()
        .filter_map(|line| {
            let id = sanitize(&line.id, MAX_ID)?;
            let text = sanitize(&line.text, MAX_TEXT)?;
            (!text.is_empty()).then_some(Item {
                id,
                t: line.t,
                text,
                ok: line.ok,
            })
        })
        .collect::<Vec<_>>();
    let mut band = band(items);
    band.items.truncate(CHATTER);
    band
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_formatting_and_invisible_characters() {
        assert_eq!(
            sanitize("§k§lhi\u{202E}there\u{200B}!", 50).unwrap(),
            "hithere!"
        );
        assert_eq!(sanitize("a\nb\tc", 50).unwrap(), "a b c");
    }

    #[test]
    fn sanitize_drops_mentions_links_and_addresses() {
        assert_eq!(
            sanitize(
                "ping <@123> see https://x.y/z?token=1 at 10.0.0.1:25565 and fe80::1:2:3",
                100
            )
            .unwrap(),
            "ping see (link) at (address) and (address)"
        );
    }

    #[test]
    fn sanitize_refuses_secret_shaped_text() {
        assert!(sanitize("token ghp_abcdefghijklmnopqrstuvwxyz012345", 100).is_none());
        assert!(sanitize("key=sk-ant-api03-xyzxyzxyzxyz", 100).is_none());
        assert!(sanitize("a8F3kq92LmZp0xYvB7nQ4wErTy", 100).is_none());
        // Ordinary long words survive.
        assert!(sanitize("runGameTestServer ElectricalAgeTransparentNodes", 100).is_some());
    }

    #[test]
    fn sanitize_clamps_by_characters() {
        let text = "é".repeat(200);
        let clean = sanitize(&text, 10).unwrap();
        assert_eq!(clean.chars().count(), 10);
        assert!(clean.ends_with('…'));
    }

    #[test]
    fn headlines_lose_emoji_and_bold() {
        assert_eq!(
            headline("📄 **Done: tests take 8 min**\nmore").unwrap(),
            "Done: tests take 8 min"
        );
        assert_eq!(
            headline("💬 **Floppy sounds: mine**").unwrap(),
            "Floppy sounds: mine"
        );
        assert!(is_noise("⚙ LabAgent · working for baughn, 3 min"));
        assert!(is_noise("🔐 **tsugumi-lab asks to run Bash**"));
        assert!(!is_noise("💬 **Floppy sounds**"));
    }

    #[test]
    fn document_from_an_empty_board() {
        let board = Board::open_in_memory().unwrap();
        let config = Config {
            agents: vec![Agent {
                id: "a".into(),
                name: "A".into(),
            }],
            file: None,
        };
        let doc = board.obelisk(&config).unwrap();
        assert_eq!(doc.agents.len(), 1);
        assert_eq!(doc.agents[0].state, "offline");
        assert!(doc.agents[0].said.items.is_empty());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("obelisk.json");
        write_file(&doc, &path).unwrap();
        let back: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(back["v"], 1);
    }
}
