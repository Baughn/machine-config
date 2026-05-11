//! `nbb-agent` runtime.
//!
//! Each agent host runs one of these. Responsibilities:
//!
//! - Listen on a TCP socket for the controller's polling connection.
//! - Watch `/var/lib/nbb/spool/*.evt` for events written by `nbb-event`.
//! - Match `Start` events to `Finish` events in memory; on a matched
//!   finish, forward an [`EventBuildFinish`] frame to the controller.
//! - Respond to `PING` with `PONG` and `TELEMETRY_GET` with `TELEMETRY`.
//!
//! Spec invariants honored here:
//!
//! - `Start` events do **not** cross the wire (they live in the agent's
//!   in-memory map until the matching finish arrives).
//! - Spool files are unlinked only **after** the agent has confirmed
//!   delivery to the controller (TCP write success); files are left in
//!   place when the controller is unreachable. This survives controller
//!   restarts.
//! - On agent restart, the in-memory start map is empty. Finishes that
//!   arrive without a matching start are forwarded with `duration_ms =
//!   None`.

use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::interval;

use crate::protocol::frame::{read_frame_async, write_frame_async, Frame};
use crate::protocol::handshake::perform_handshake_async;
use crate::protocol::ops::{op, AgentHello, EventBuildFinish, SpoolEvent, TelemetryBody};
use crate::telemetry::{self, Telemetry};
use crate::util::now_ms;

#[derive(Clone, Debug)]
pub struct AgentConfig {
    pub bind_addr: SocketAddr,
    pub spool_dir: PathBuf,
    pub hostname: String,
    pub system: String,
    pub capacity: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingStart {
    pub pname: String,
    pub ts_ms: u64,
}

/// Pure logic decision returned by [`apply_event`]. The watcher decides
/// whether to unlink the spool file based on which variant it sees and the
/// result of forwarding (when applicable).
#[derive(Clone, Debug, PartialEq)]
pub enum ApplyOutcome {
    StoredStart,
    ForwardFinish(EventBuildFinish),
}

/// Apply one spool event to the agent's in-memory start map. Pure function
/// for testability — the watcher handles I/O around it.
pub fn apply_event(
    pending_starts: &mut HashMap<String, PendingStart>,
    event: SpoolEvent,
) -> ApplyOutcome {
    match event {
        SpoolEvent::Start {
            drv_path,
            pname,
            host: _,
            ts_ms,
        } => {
            pending_starts.insert(drv_path, PendingStart { pname, ts_ms });
            ApplyOutcome::StoredStart
        }
        SpoolEvent::Finish {
            drv_path,
            pname,
            host,
            ts_ms,
            status,
            out_paths,
        } => {
            let started = pending_starts.remove(&drv_path);
            let duration_ms = started.as_ref().and_then(|s| ts_ms.checked_sub(s.ts_ms));
            let pname = started.map(|s| s.pname).unwrap_or(pname);
            ApplyOutcome::ForwardFinish(EventBuildFinish {
                drv_path,
                pname,
                host,
                ts_ms,
                duration_ms,
                status,
                out_paths,
            })
        }
    }
}

struct AgentState {
    config: AgentConfig,
    pending_starts: HashMap<String, PendingStart>,
    writer: Option<Arc<ConnectionWriter>>,
}

struct ConnectionWriter {
    inner: AsyncMutex<OwnedWriteHalf>,
}

impl ConnectionWriter {
    async fn write_frame(&self, frame: &Frame) -> io::Result<()> {
        let mut g = self.inner.lock().await;
        write_frame_async(&mut *g, frame).await
    }
}

pub async fn run(config: AgentConfig) -> io::Result<()> {
    std::fs::create_dir_all(&config.spool_dir).ok();
    let listener = TcpListener::bind(config.bind_addr).await?;
    tracing::info!(
        addr = %config.bind_addr,
        host = %config.hostname,
        system = %config.system,
        "nbb-agent listening"
    );

    let state = Arc::new(Mutex::new(AgentState {
        config,
        pending_starts: HashMap::new(),
        writer: None,
    }));

    let watcher_state = Arc::clone(&state);
    tokio::spawn(async move {
        spool_watcher_loop(watcher_state, Duration::from_secs(1)).await;
    });

    loop {
        let (stream, peer) = listener.accept().await?;
        tracing::info!(?peer, "controller connected");
        let conn_state = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(err) = handle_connection(stream, conn_state).await {
                tracing::warn!(?err, "controller connection ended");
            }
        });
    }
}

async fn handle_connection(mut stream: TcpStream, state: Arc<Mutex<AgentState>>) -> io::Result<()> {
    perform_handshake_async(&mut stream).await?;

    let (mut reader, write_half) = stream.into_split();
    let writer = Arc::new(ConnectionWriter {
        inner: AsyncMutex::new(write_half),
    });

    let hello = {
        let s = state.lock().expect("agent state mutex");
        AgentHello {
            name: s.config.hostname.clone(),
            system: s.config.system.clone(),
            capacity: s.config.capacity,
        }
    };
    writer
        .write_frame(&Frame::with_body(op::AGENT_HELLO, &hello)?)
        .await?;

    state.lock().expect("agent state mutex").writer = Some(Arc::clone(&writer));

    let result = loop {
        let frame = match read_frame_async(&mut reader).await {
            Ok(f) => f,
            Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => break Ok(()),
            Err(err) => break Err(err),
        };
        match frame.op_id {
            op::PING => {
                if let Err(err) = writer.write_frame(&Frame::empty(op::PONG)).await {
                    break Err(err);
                }
            }
            op::TELEMETRY_GET => {
                let body = match telemetry::sample() {
                    Ok(t) => to_telemetry_body(&t),
                    Err(err) => {
                        tracing::warn!(?err, "telemetry sample failed");
                        TelemetryBody {
                            mem_available_kb: 0,
                            psi_memory_some_avg10: None,
                            nix_slots_active: 0,
                            sampled_at_ms: now_ms_u64(),
                        }
                    }
                };
                let frame = match Frame::with_body(op::TELEMETRY, &body) {
                    Ok(f) => f,
                    Err(err) => {
                        tracing::error!(?err, "encoding TELEMETRY");
                        continue;
                    }
                };
                if let Err(err) = writer.write_frame(&frame).await {
                    break Err(err);
                }
            }
            other => {
                tracing::warn!(op = other, "agent received unexpected op_id");
            }
        }
    };

    state.lock().expect("agent state mutex").writer = None;
    result
}

async fn spool_watcher_loop(state: Arc<Mutex<AgentState>>, period: Duration) {
    let mut ticker = interval(period);
    loop {
        ticker.tick().await;
        if let Err(err) = tick(&state).await {
            tracing::warn!(?err, "spool watcher tick failed");
        }
    }
}

async fn tick(state: &Arc<Mutex<AgentState>>) -> io::Result<()> {
    let spool_dir = state
        .lock()
        .expect("agent state mutex")
        .config
        .spool_dir
        .clone();
    let mut entries: Vec<PathBuf> = match std::fs::read_dir(&spool_dir) {
        Ok(iter) => iter.flatten().map(|e| e.path()).collect(),
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    entries.sort();

    for path in entries {
        if path.extension().and_then(|s| s.to_str()) != Some("evt") {
            continue;
        }
        match process_one(state, &path).await {
            Ok(true) => {
                let _ = std::fs::remove_file(&path);
            }
            Ok(false) => {
                // Controller unreachable; retry next tick.
            }
            Err(err) => {
                tracing::warn!(path = %path.display(), ?err, "corrupt spool entry; removing");
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    Ok(())
}

async fn process_one(state: &Arc<Mutex<AgentState>>, path: &Path) -> io::Result<bool> {
    let bytes = std::fs::read(path)?;
    let (event, _) = bincode::decode_from_slice::<SpoolEvent, _>(
        &bytes,
        crate::protocol::frame::bincode_config(),
    )
    .map_err(io::Error::other)?;

    let outcome = {
        let mut s = state.lock().expect("agent state mutex");
        apply_event(&mut s.pending_starts, event)
    };

    match outcome {
        ApplyOutcome::StoredStart => Ok(true),
        ApplyOutcome::ForwardFinish(event) => forward_finish(state, &event).await,
    }
}

async fn forward_finish(
    state: &Arc<Mutex<AgentState>>,
    event: &EventBuildFinish,
) -> io::Result<bool> {
    let writer = state.lock().expect("agent state mutex").writer.clone();
    let Some(writer) = writer else {
        return Ok(false);
    };
    let frame = Frame::with_body(op::EVENT_BUILD_FINISH, event)?;
    match writer.write_frame(&frame).await {
        Ok(()) => Ok(true),
        Err(err) => {
            tracing::warn!(?err, "EVENT_BUILD_FINISH push failed; leaving spool entry");
            state.lock().expect("agent state mutex").writer = None;
            Ok(false)
        }
    }
}

fn to_telemetry_body(t: &Telemetry) -> TelemetryBody {
    TelemetryBody {
        mem_available_kb: t.mem_available_kb,
        psi_memory_some_avg10: t.psi_memory_some_avg10,
        nix_slots_active: u32::try_from(t.nix_slots_active).unwrap_or(u32::MAX),
        sampled_at_ms: u64::try_from(t.sampled_at_ms).unwrap_or(u64::MAX),
    }
}

fn now_ms_u64() -> u64 {
    u64::try_from(now_ms()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ops::BuildStatus;

    fn start(drv: &str, pname: &str, ts_ms: u64) -> SpoolEvent {
        SpoolEvent::Start {
            drv_path: drv.to_string(),
            pname: pname.to_string(),
            host: "tsugumi".to_string(),
            ts_ms,
        }
    }

    fn finish(drv: &str, pname: &str, ts_ms: u64) -> SpoolEvent {
        SpoolEvent::Finish {
            drv_path: drv.to_string(),
            pname: pname.to_string(),
            host: "tsugumi".to_string(),
            ts_ms,
            status: BuildStatus::Success,
            out_paths: vec!["/nix/store/out".to_string()],
        }
    }

    #[test]
    fn start_then_finish_yields_duration() {
        let mut pending = HashMap::new();
        assert_eq!(
            apply_event(&mut pending, start("/d.drv", "foo", 100)),
            ApplyOutcome::StoredStart
        );
        let outcome = apply_event(&mut pending, finish("/d.drv", "foo", 500));
        let ApplyOutcome::ForwardFinish(event) = outcome else {
            panic!("expected forward");
        };
        assert_eq!(event.duration_ms, Some(400));
        assert_eq!(event.pname, "foo");
        assert!(pending.is_empty(), "start should be consumed");
    }

    #[test]
    fn finish_without_matching_start_forwards_with_no_duration() {
        let mut pending = HashMap::new();
        let outcome = apply_event(&mut pending, finish("/d.drv", "foo", 500));
        let ApplyOutcome::ForwardFinish(event) = outcome else {
            panic!("expected forward");
        };
        assert_eq!(event.duration_ms, None);
        assert_eq!(event.pname, "foo");
    }

    #[test]
    fn finish_with_earlier_ts_than_start_clamps_to_none() {
        // Clock skew or out-of-order events: ts_ms_finish < ts_ms_start.
        // duration_ms should be None (no negative durations).
        let mut pending = HashMap::new();
        apply_event(&mut pending, start("/d.drv", "foo", 1000));
        let ApplyOutcome::ForwardFinish(event) =
            apply_event(&mut pending, finish("/d.drv", "foo", 500))
        else {
            panic!("expected forward");
        };
        assert_eq!(event.duration_ms, None);
    }

    #[test]
    fn pname_preferred_from_start_when_available() {
        let mut pending = HashMap::new();
        apply_event(&mut pending, start("/d.drv", "canonical-pname", 100));
        let ApplyOutcome::ForwardFinish(event) = apply_event(
            &mut pending,
            finish("/d.drv", "other-pname-the-finish-computed", 500),
        ) else {
            panic!("expected forward");
        };
        assert_eq!(event.pname, "canonical-pname");
    }

    #[test]
    fn multiple_starts_do_not_interfere() {
        let mut pending = HashMap::new();
        apply_event(&mut pending, start("/a.drv", "a", 100));
        apply_event(&mut pending, start("/b.drv", "b", 200));
        let ApplyOutcome::ForwardFinish(event_b) =
            apply_event(&mut pending, finish("/b.drv", "b", 800))
        else {
            panic!("expected forward");
        };
        assert_eq!(event_b.duration_ms, Some(600));
        let ApplyOutcome::ForwardFinish(event_a) =
            apply_event(&mut pending, finish("/a.drv", "a", 600))
        else {
            panic!("expected forward");
        };
        assert_eq!(event_a.duration_ms, Some(500));
    }

    #[test]
    fn tick_processes_files_in_ulid_order() {
        // ULID-style filenames lex-sort by time. Write Start with later
        // filename and Finish with earlier filename; the watcher must
        // process Start first because we ordered the entries correctly.
        // In reality the writer assigns ULIDs in time order; we just make
        // sure the sort is happening.
        let dir = tempdir();
        std::fs::create_dir_all(&dir).unwrap();

        let cfg_path = dir.clone();
        let state = Arc::new(Mutex::new(AgentState {
            config: AgentConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                spool_dir: cfg_path,
                hostname: "tsugumi".into(),
                system: "x86_64-linux".into(),
                capacity: 1,
            },
            pending_starts: HashMap::new(),
            writer: None,
        }));

        // Earlier ULID = Start; later ULID = Finish.
        write_spool(&dir, "01HAA-start", &start("/d.drv", "foo", 100));
        write_spool(&dir, "01HAB-finish", &finish("/d.drv", "foo", 500));

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            // No writer → finish forward fails → file stays.
            tick(&state).await.unwrap();
        });

        // Start should be processed and unlinked.
        assert!(!dir.join("01HAA-start.evt").exists());
        // Finish stays (no controller to forward to).
        assert!(dir.join("01HAB-finish.evt").exists());
        // pending_starts should be empty: start was consumed when finish
        // was processed (and finish should have moved to forward).
        assert!(state.lock().unwrap().pending_starts.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tick_corrupt_file_is_removed() {
        let dir = tempdir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("bad.evt"), b"\xff\xff\xff\xff garbage").unwrap();

        let state = Arc::new(Mutex::new(AgentState {
            config: AgentConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                spool_dir: dir.clone(),
                hostname: "tsugumi".into(),
                system: "x86_64-linux".into(),
                capacity: 1,
            },
            pending_starts: HashMap::new(),
            writer: None,
        }));
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            tick(&state).await.unwrap();
        });
        assert!(!dir.join("bad.evt").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tick_skips_non_evt_files() {
        let dir = tempdir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("foo.tmp"), b"in-flight write").unwrap();
        let state = Arc::new(Mutex::new(AgentState {
            config: AgentConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                spool_dir: dir.clone(),
                hostname: "tsugumi".into(),
                system: "x86_64-linux".into(),
                capacity: 1,
            },
            pending_starts: HashMap::new(),
            writer: None,
        }));
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            tick(&state).await.unwrap();
        });
        assert!(dir.join("foo.tmp").exists(), ".tmp must not be deleted");

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn write_spool(dir: &Path, name: &str, event: &SpoolEvent) {
        let bytes =
            bincode::encode_to_vec(event, crate::protocol::frame::bincode_config()).unwrap();
        std::fs::write(dir.join(format!("{name}.evt")), bytes).unwrap();
    }

    fn tempdir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "nbb-agent-test-{}-{}-{n}",
            std::process::id(),
            now_ms()
        ))
    }
}
