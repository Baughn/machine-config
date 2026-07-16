//! Parser and state model for nix's `--log-format internal-json` stream.

use std::collections::{BTreeMap, VecDeque};
use std::time::Instant;

use serde::Deserialize;

use crate::diff::store_name;

// Activity and result type ids from nix's logging.hh.
const ACT_FILE_TRANSFER: u64 = 101;
const ACT_BUILDS: u64 = 104;
const ACT_BUILD: u64 = 105;

const RES_BUILD_LOG_LINE: u64 = 101;
const RES_SET_PHASE: u64 = 104;
const RES_PROGRESS: u64 = 105;
const RES_SET_EXPECTED: u64 = 106;

const LVL_ERROR: u64 = 0;
const LVL_WARN: u64 = 1;

const LOG_TAIL_LINES: usize = 3;
const RECENT_BUILDS: usize = 5;

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "lowercase")]
enum Event {
    Start {
        id: u64,
        #[serde(rename = "type")]
        kind: u64,
        #[serde(default)]
        text: String,
        #[serde(default)]
        fields: Vec<Field>,
    },
    Stop {
        id: u64,
    },
    Result {
        id: u64,
        #[serde(rename = "type")]
        kind: u64,
        #[serde(default)]
        fields: Vec<Field>,
    },
    Msg {
        level: u64,
        msg: String,
    },
}

/// Activity fields are heterogeneous arrays of ints and strings.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Field {
    Int(u64),
    Str(String),
}

impl Field {
    fn as_int(&self) -> Option<u64> {
        match self {
            Field::Int(n) => Some(*n),
            Field::Str(_) => None,
        }
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            Field::Int(_) => None,
            Field::Str(s) => Some(s),
        }
    }
}

/// A derivation currently building.
#[derive(Debug)]
pub struct Build {
    pub name: String,
    pub started: Instant,
    pub phase: Option<String>,
    pub log_tail: VecDeque<String>,
}

/// A download or path copy in flight.
#[derive(Debug)]
pub struct Transfer {
    pub label: String,
    pub done_bytes: u64,
    pub expected_bytes: u64,
}

/// A recently finished build, for the "recent" display line.
#[derive(Debug)]
pub struct Finished {
    pub name: String,
    pub secs: f64,
}

/// Aggregated build state, updated line-by-line by the parser thread and
/// snapshotted by the render thread.
#[derive(Debug)]
pub struct Model {
    pub started: Instant,
    pub running: BTreeMap<u64, Build>,
    pub transfers: BTreeMap<u64, Transfer>,
    pub recent: VecDeque<Finished>,
    pub builds_done: u64,
    pub builds_expected: u64,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    builds_aggregate_id: Option<u64>,
}

impl Model {
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
            running: BTreeMap::new(),
            transfers: BTreeMap::new(),
            recent: VecDeque::new(),
            builds_done: 0,
            builds_expected: 0,
            errors: Vec::new(),
            warnings: Vec::new(),
            builds_aggregate_id: None,
        }
    }

    /// Feed one raw stderr line. Non-`@nix` lines and unparsable JSON are
    /// ignored; nix emits nothing else in internal-json mode.
    pub fn apply_line(&mut self, line: &str) {
        let Some(json) = line.strip_prefix("@nix ") else {
            return;
        };
        let Ok(event) = serde_json::from_str::<Event>(json) else {
            return;
        };
        self.apply(event);
    }

    fn apply(&mut self, event: Event) {
        match event {
            Event::Start {
                id,
                kind,
                text,
                fields,
            } => self.start(id, kind, &text, &fields),
            Event::Stop { id } => self.stop(id),
            Event::Result { id, kind, fields } => self.result(id, kind, &fields),
            Event::Msg { level, msg } => match level {
                LVL_ERROR => self.errors.push(msg),
                LVL_WARN => self.warnings.push(msg),
                _ => {}
            },
        }
    }

    fn start(&mut self, id: u64, kind: u64, text: &str, fields: &[Field]) {
        match kind {
            ACT_BUILD => {
                let name = fields
                    .first()
                    .and_then(Field::as_str)
                    .map(|drv| store_name(drv).trim_end_matches(".drv").to_string())
                    .unwrap_or_else(|| text.to_string());
                self.running.insert(
                    id,
                    Build {
                        name,
                        started: Instant::now(),
                        phase: None,
                        log_tail: VecDeque::new(),
                    },
                );
            }
            ACT_FILE_TRANSFER => {
                let label = fields
                    .first()
                    .and_then(Field::as_str)
                    .map(|uri| uri.rsplit('/').next().unwrap_or(uri).to_string())
                    .unwrap_or_else(|| text.to_string());
                self.transfers.insert(
                    id,
                    Transfer {
                        label,
                        done_bytes: 0,
                        expected_bytes: 0,
                    },
                );
            }
            ACT_BUILDS => self.builds_aggregate_id = Some(id),
            _ => {}
        }
    }

    fn stop(&mut self, id: u64) {
        if let Some(build) = self.running.remove(&id) {
            self.recent.push_front(Finished {
                name: build.name,
                secs: build.started.elapsed().as_secs_f64(),
            });
            self.recent.truncate(RECENT_BUILDS);
        }
        self.transfers.remove(&id);
        if self.builds_aggregate_id == Some(id) {
            self.builds_aggregate_id = None;
        }
    }

    fn result(&mut self, id: u64, kind: u64, fields: &[Field]) {
        match kind {
            RES_BUILD_LOG_LINE => {
                if let (Some(build), Some(line)) = (
                    self.running.get_mut(&id),
                    fields.first().and_then(Field::as_str),
                ) {
                    build.log_tail.push_back(line.trim_end().to_string());
                    while build.log_tail.len() > LOG_TAIL_LINES {
                        build.log_tail.pop_front();
                    }
                }
            }
            RES_SET_PHASE => {
                if let (Some(build), Some(phase)) = (
                    self.running.get_mut(&id),
                    fields.first().and_then(Field::as_str),
                ) {
                    build.phase = Some(phase.to_string());
                }
            }
            RES_PROGRESS => {
                let ints: Vec<u64> = fields.iter().filter_map(Field::as_int).collect();
                if self.builds_aggregate_id == Some(id) {
                    if let [done, expected, ..] = ints[..] {
                        self.builds_done = done;
                        self.builds_expected = expected;
                    }
                } else if let Some(transfer) = self.transfers.get_mut(&id) {
                    if let [done, expected, ..] = ints[..] {
                        transfer.done_bytes = done;
                        transfer.expected_bytes = expected;
                    }
                }
            }
            // fields = [activity type, expected count] on the aggregate.
            RES_SET_EXPECTED if self.builds_aggregate_id == Some(id) => {
                if let (Some(ACT_BUILD), Some(n)) = (
                    fields.first().and_then(Field::as_int),
                    fields.get(1).and_then(Field::as_int),
                ) {
                    self.builds_expected = self.builds_expected.max(n);
                }
            }
            _ => {}
        }
    }
}

impl Default for Model {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(model: &mut Model, lines: &[&str]) {
        for line in lines {
            model.apply_line(line);
        }
    }

    #[test]
    fn tracks_build_lifecycle() {
        let mut m = Model::new();
        feed(
            &mut m,
            &[
                r#"@nix {"action":"start","id":1,"level":3,"parent":0,"text":"Realise","type":102}"#,
                r#"@nix {"action":"start","id":2,"level":3,"parent":1,"text":"builds","type":104}"#,
                r#"@nix {"action":"start","id":3,"level":3,"parent":2,"text":"building kwin","fields":["/nix/store/abcdefghijklmnopqrstuvwxyz012345-kwin-6.6.3.drv","",1,1],"type":105}"#,
                r#"@nix {"action":"result","id":3,"type":104,"fields":["buildPhase"]}"#,
                r#"@nix {"action":"result","id":3,"type":101,"fields":["ninja: [1/100]"]}"#,
                r#"@nix {"action":"result","id":2,"type":105,"fields":[1,40,3,0]}"#,
            ],
        );
        assert_eq!(m.running.len(), 1);
        let build = m.running.values().next().unwrap();
        assert_eq!(build.name, "kwin-6.6.3");
        assert_eq!(build.phase.as_deref(), Some("buildPhase"));
        assert_eq!(
            build.log_tail.back().map(String::as_str),
            Some("ninja: [1/100]")
        );
        assert_eq!((m.builds_done, m.builds_expected), (1, 40));

        m.apply_line(r#"@nix {"action":"stop","id":3}"#);
        assert!(m.running.is_empty());
        assert_eq!(
            m.recent.front().map(|f| f.name.as_str()),
            Some("kwin-6.6.3")
        );
    }

    #[test]
    fn tracks_transfers_and_messages() {
        let mut m = Model::new();
        feed(
            &mut m,
            &[
                r#"@nix {"action":"start","id":7,"level":3,"parent":0,"text":"","fields":["https://cache.nixos.org/nar/xyz.nar.xz"],"type":101}"#,
                r#"@nix {"action":"result","id":7,"type":105,"fields":[1024,4096,0,0]}"#,
                r#"@nix {"action":"msg","level":0,"msg":"error: builder failed"}"#,
                r#"@nix {"action":"msg","level":1,"msg":"warning: dirty tree"}"#,
                "not json at all",
            ],
        );
        let t = m.transfers.values().next().unwrap();
        assert_eq!(t.label, "xyz.nar.xz");
        assert_eq!((t.done_bytes, t.expected_bytes), (1024, 4096));
        assert_eq!(m.errors, ["error: builder failed"]);
        assert_eq!(m.warnings, ["warning: dirty tree"]);

        m.apply_line(r#"@nix {"action":"stop","id":7}"#);
        assert!(m.transfers.is_empty());
    }
}
