//! `nbb-controller` runtime.
//!
//! One controller process per deployment. Responsibilities:
//!
//! - Maintain a long-lived TCP connection to each agent. Send `PING` and
//!   `TELEMETRY_GET` on a poll interval; receive `PONG`, `TELEMETRY`, and
//!   unsolicited `EVENT_BUILD_FINISH` pushes.
//! - Accept Unix-socket connections from `nbb-hook` and reply
//!   `DECIDE_CANDIDATE → DECISION`; record matching `Admission` rows.
//!   Handle later `ADMISSION_FINISH` arrivals on the same protocol.
//! - Run a 5-second watchdog that retires admissions via:
//!     1. Sentinel sweep (`/run/nbb/inflight/*`): if the hook PID is
//!        `ESRCH`, retire the admission and unlink the sentinel.
//!     2. Wall-clock TTL: anything older than `max(predicted_ms × 2,
//!        60_000)` is retired regardless.
//! - Own the SQLite database. Clears the `admissions` table on startup.
//!
//! Spec notes: SOCK_SEQPACKET was specified for the hook socket, but
//! length-prefixed framing makes ordinary SOCK_STREAM equally safe and
//! tokio supports it out of the box. The transport is a `UnixStream`.

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use rusqlite::Connection;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpStream, UnixListener};
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::{interval, MissedTickBehavior};

use crate::inflight::{pid_is_dead, read_sentinel};
use crate::persistence::{self, admissions, observations};
use crate::protocol::frame::{read_frame_async, write_frame_async, Frame};
use crate::protocol::handshake::perform_handshake_async;
use crate::protocol::ops::{
    op, AdmissionFinish, AgentHello, DecideCandidate, Decision, EventBuildFinish, TelemetryBody,
};
use crate::scheduler::{
    self, SchedulerDecision, SchedulerInputs, SchedulerPolicy, Target, TargetState,
};
pub use crate::util::now_ms_u64;
use crate::util::pname_from_drv;

#[derive(Clone, Debug)]
pub struct ControllerConfig {
    pub system: String,
    pub data_dir: PathBuf,
    pub inflight_dir: PathBuf,
    pub hook_socket: PathBuf,
    pub targets: Vec<Target>,
    pub poll_interval: Duration,
    pub policy: SchedulerPolicy,
    pub max_samples_per_pname: u32,
}

/// Tracked liveness per target. Updated by the target poller, read by the
/// scheduler and watchdog.
#[derive(Clone, Debug, Default)]
pub struct TargetRuntime {
    pub last_pong_ms: Option<u64>,
    pub last_telemetry: Option<TelemetryBody>,
}

pub struct ControllerState {
    pub config: ControllerConfig,
    pub conn: AsyncMutex<Connection>,
    pub target_runtimes: std::sync::Mutex<HashMap<String, TargetRuntime>>,
}

impl ControllerState {
    pub fn build_target_states(&self) -> Vec<TargetState> {
        let runtimes = self.target_runtimes.lock().expect("target_runtimes");
        self.config
            .targets
            .iter()
            .map(|t| {
                let rt = runtimes.get(&t.name).cloned().unwrap_or_default();
                TargetState {
                    target: t.clone(),
                    last_pong_ms: rt.last_pong_ms,
                    last_telemetry: rt.last_telemetry,
                }
            })
            .collect()
    }
}

pub async fn open_state(config: ControllerConfig) -> io::Result<Arc<ControllerState>> {
    let db_path = config.data_dir.join("state.db");
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let conn = persistence::open(&db_path)?;
    persistence::clear_admissions(&conn)?;
    let target_runtimes: HashMap<String, TargetRuntime> = config
        .targets
        .iter()
        .map(|t| (t.name.clone(), TargetRuntime::default()))
        .collect();
    Ok(Arc::new(ControllerState {
        config,
        conn: AsyncMutex::new(conn),
        target_runtimes: std::sync::Mutex::new(target_runtimes),
    }))
}

pub async fn run(config: ControllerConfig) -> io::Result<()> {
    let state = open_state(config).await?;
    tracing::info!(
        targets = ?state.config.targets.iter().map(|t| &t.name).collect::<Vec<_>>(),
        "nbb-controller starting"
    );

    // Spawn one poller per target.
    for target in &state.config.targets {
        let target = target.clone();
        let state = Arc::clone(&state);
        tokio::spawn(async move { target_poller_loop(target, state).await });
    }

    // Hook socket listener.
    let hook_state = Arc::clone(&state);
    tokio::spawn(async move {
        if let Err(err) = hook_socket_listener(hook_state).await {
            tracing::error!(?err, "hook socket listener exited");
        }
    });

    // Watchdog.
    let wd_state = Arc::clone(&state);
    tokio::spawn(async move { watchdog_loop(wd_state).await });

    tokio::signal::ctrl_c().await?;
    tracing::info!("nbb-controller shutting down");
    Ok(())
}

async fn target_poller_loop(target: Target, state: Arc<ControllerState>) {
    let mut backoff = Duration::from_millis(500);
    loop {
        let outcome = match TcpStream::connect(target.tcp_addr).await {
            Ok(stream) => run_target_session(stream, &target, &state).await,
            Err(err) => Err(err),
        };
        if let Err(err) = outcome {
            tracing::warn!(target = %target.name, ?err, "agent connection ended");
            // Clear runtime so the scheduler considers this target stale.
            state
                .target_runtimes
                .lock()
                .expect("target_runtimes")
                .insert(target.name.clone(), TargetRuntime::default());
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

pub async fn run_target_session<S>(
    mut stream: S,
    target: &Target,
    state: &Arc<ControllerState>,
) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    perform_handshake_async(&mut stream).await?;

    let hello_frame = read_frame_async(&mut stream).await?;
    if hello_frame.op_id != op::AGENT_HELLO {
        return Err(io::Error::other(format!(
            "expected AGENT_HELLO from {}, got op_id {}",
            target.name, hello_frame.op_id
        )));
    }
    let _hello: AgentHello = hello_frame.decode_body()?;
    tracing::info!(target = %target.name, "agent handshake complete");

    let mut ticker = interval(state.config.poll_interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                write_frame_async(&mut stream, &Frame::empty(op::PING)).await?;
                write_frame_async(&mut stream, &Frame::empty(op::TELEMETRY_GET)).await?;
            }
            frame_result = read_frame_async(&mut stream) => {
                match frame_result {
                    Ok(frame) => handle_from_agent(&target.name, frame, state).await?,
                    Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
                    Err(err) => return Err(err),
                }
            }
        }
    }
}

async fn handle_from_agent(
    target_name: &str,
    frame: Frame,
    state: &Arc<ControllerState>,
) -> io::Result<()> {
    match frame.op_id {
        op::PONG => {
            let now = now_ms_u64();
            let mut runtimes = state.target_runtimes.lock().expect("target_runtimes");
            runtimes
                .entry(target_name.to_string())
                .or_default()
                .last_pong_ms = Some(now);
        }
        op::TELEMETRY => {
            let body: TelemetryBody = frame.decode_body()?;
            let mut runtimes = state.target_runtimes.lock().expect("target_runtimes");
            runtimes
                .entry(target_name.to_string())
                .or_default()
                .last_telemetry = Some(body);
        }
        op::EVENT_BUILD_FINISH => {
            let event: EventBuildFinish = frame.decode_body()?;
            record_finish(state, event).await?;
        }
        other => {
            tracing::warn!(target = %target_name, op = other, "controller got unexpected op");
        }
    }
    Ok(())
}

/// Apply one EVENT_BUILD_FINISH: maybe write an observation row (only when
/// `duration_ms` is `Some`), and unconditionally retire the matching
/// admission.
pub async fn record_finish(
    state: &Arc<ControllerState>,
    event: EventBuildFinish,
) -> io::Result<()> {
    let max = state.config.max_samples_per_pname;
    let drv = event.drv_path.clone();
    let conn = state.conn.lock().await;
    let wrote = observations::record_finish(&conn, &event, max)?;
    admissions::retire(&conn, &drv)?;
    drop(conn);
    if wrote {
        tracing::info!(
            host = %event.host,
            pname = %event.pname,
            drv = %drv,
            duration_ms = event.duration_ms.unwrap_or(0),
            status = event.status.as_str(),
            "observation recorded"
        );
    } else if event.duration_ms.is_none() {
        tracing::info!(
            drv = %drv,
            "EVENT_BUILD_FINISH without duration; admission retired, no observation row"
        );
    } else {
        tracing::info!(
            host = %event.host,
            pname = %event.pname,
            drv = %drv,
            "duplicate EVENT_BUILD_FINISH; admission retired, no new observation row"
        );
    }
    Ok(())
}

async fn hook_socket_listener(state: Arc<ControllerState>) -> io::Result<()> {
    let path = state.config.hook_socket.clone();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    tracing::info!(socket = %path.display(), "hook socket listening");
    loop {
        let (stream, _addr) = listener.accept().await?;
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(err) = handle_hook_connection(stream, state).await {
                tracing::warn!(?err, "hook connection ended");
            }
        });
    }
}

pub async fn handle_hook_connection<S>(mut stream: S, state: Arc<ControllerState>) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    perform_handshake_async(&mut stream).await?;

    loop {
        let frame = match read_frame_async(&mut stream).await {
            Ok(f) => f,
            Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(err) => return Err(err),
        };
        match frame.op_id {
            op::DECIDE_CANDIDATE => {
                let candidate: DecideCandidate = frame.decode_body()?;
                let decision = make_decision(&state, &candidate).await?;
                let reply = Frame::with_body(op::DECISION, &decision)?;
                write_frame_async(&mut stream, &reply).await?;
            }
            op::ADMISSION_FINISH => {
                let finish: AdmissionFinish = frame.decode_body()?;
                let conn = state.conn.lock().await;
                admissions::retire(&conn, &finish.drv_path)?;
            }
            other => {
                tracing::warn!(op = other, "hook sent unexpected op");
            }
        }
    }
}

pub async fn make_decision(
    state: &Arc<ControllerState>,
    candidate: &DecideCandidate,
) -> io::Result<Decision> {
    let pname = pname_from_drv(&candidate.drv_path);
    let (p95, admissions_rows) = {
        let conn = state.conn.lock().await;
        (
            observations::p95_ms(&conn, &pname)?,
            admissions::list(&conn)?,
        )
    };

    let target_states = state.build_target_states();
    let inputs = SchedulerInputs {
        system: &state.config.system,
        candidate,
        now_ms: now_ms_u64(),
        poll_interval_ms: state
            .config
            .poll_interval
            .as_millis()
            .min(u128::from(u64::MAX)) as u64,
        policy: &state.config.policy,
        admissions: &admissions_rows,
        targets: &target_states,
        p95_for_pname: p95,
    };

    match scheduler::decide(&inputs) {
        SchedulerDecision::Decline => {
            tracing::info!(
                drv = %candidate.drv_path,
                pname = %pname,
                system = %candidate.system,
                p95_ms = ?p95,
                "decision: decline"
            );
            Ok(Decision::Decline)
        }
        SchedulerDecision::RouteLocal {
            target_name,
            predicted_ms,
        } => {
            let conn = state.conn.lock().await;
            admissions::record(
                &conn,
                &candidate.drv_path,
                &target_name,
                now_ms_u64(),
                predicted_ms,
            )?;
            drop(conn);
            tracing::info!(
                drv = %candidate.drv_path,
                pname = %pname,
                target = %target_name,
                predicted_ms,
                p95_ms = ?p95,
                "decision: route-local (admission recorded; nix builds locally)"
            );
            Ok(Decision::Decline)
        }
        SchedulerDecision::Accept {
            target,
            predicted_ms,
        } => {
            let conn = state.conn.lock().await;
            admissions::record(
                &conn,
                &candidate.drv_path,
                &target.name,
                now_ms_u64(),
                predicted_ms,
            )?;
            drop(conn);
            tracing::info!(
                drv = %candidate.drv_path,
                pname = %pname,
                target = %target.name,
                predicted_ms,
                p95_ms = ?p95,
                "decision: accept"
            );
            Ok(Decision::Accept { target })
        }
    }
}

async fn watchdog_loop(state: Arc<ControllerState>) {
    let mut ticker = interval(Duration::from_secs(5));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        ticker.tick().await;
        if let Err(err) = watchdog_tick(&state).await {
            tracing::warn!(?err, "watchdog tick failed");
        }
    }
}

/// One iteration of the watchdog. Public so the lifecycle tests can drive
/// it deterministically.
pub async fn watchdog_tick(state: &Arc<ControllerState>) -> io::Result<()> {
    sweep_sentinels(state).await?;
    sweep_wall_clock_ttl(state).await?;
    Ok(())
}

async fn sweep_sentinels(state: &Arc<ControllerState>) -> io::Result<()> {
    let dir = state.config.inflight_dir.clone();
    let entries: Vec<PathBuf> = match std::fs::read_dir(&dir) {
        Ok(iter) => iter.flatten().map(|e| e.path()).collect(),
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    for path in entries {
        let Ok(sentinel) = read_sentinel(&path) else {
            // Corrupt sentinel — unlink so it doesn't wedge the sweep.
            let _ = std::fs::remove_file(&path);
            continue;
        };
        if pid_is_dead(sentinel.pid) {
            tracing::warn!(
                drv = %sentinel.drv_path,
                pid = sentinel.pid,
                "sentinel PID is dead; retiring admission"
            );
            let conn = state.conn.lock().await;
            admissions::retire(&conn, &sentinel.drv_path)?;
            drop(conn);
            let _ = std::fs::remove_file(&path);
        }
    }
    Ok(())
}

async fn sweep_wall_clock_ttl(state: &Arc<ControllerState>) -> io::Result<()> {
    let now = now_ms_u64();
    let conn = state.conn.lock().await;
    let stale = admissions::stale_drvs(&conn, now)?;
    for drv in stale {
        tracing::warn!(drv = %drv, "wall-clock TTL retiring stale admission");
        admissions::retire(&conn, &drv)?;
    }
    Ok(())
}
