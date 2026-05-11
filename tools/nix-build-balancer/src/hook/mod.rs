//! `nbb-hook` — the Nix build-hook process.
//!
//! Invoked by Nix's `build-hook` setting on the controller host. For each
//! `try` candidate received from Nix on stdin, the hook asks the local
//! controller (over a Unix socket) whether to delegate. On `Decline` the
//! hook writes `# decline` to stderr and loops. On `Accept` it spawns
//! `nix __build-remote` with the controller-supplied builder line and
//! proxies the protocol through.
//!
//! Sentinel lifecycle (SPEC §"Build observation lifecycle" item 4): the
//! hook writes `/run/nbb/inflight/<drv_hash>` after accept and unlinks it
//! on every exit path via a `Drop` guard. The controller's watchdog sweeps
//! these to retire admissions for crashed hooks.

pub mod candidate;
pub mod delegate;

use std::io;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use crate::inflight::{self, Sentinel};
use crate::protocol::frame::{read_frame_sync, write_frame_sync, Frame};
use crate::protocol::handshake::perform_handshake_sync;
use crate::protocol::ops::{op, AdmissionFinish, BuildStatus, DecideCandidate, Decision};
use crate::util::now_ms_u64;

use candidate::{read_hook_candidate, read_hook_settings, HookCandidate};
use delegate::delegate_remote_build;

#[derive(Clone, Debug)]
pub struct HookConfig {
    pub controller_socket: PathBuf,
    pub inflight_dir: PathBuf,
    pub nix_bin: PathBuf,
    pub verbosity: String,
}

/// Top-level hook loop driven by Nix's stdin.
pub fn run_hook(cfg: HookConfig) -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    let settings = read_hook_settings(&mut stdin)?;

    loop {
        let Some(candidate) = read_hook_candidate(&mut stdin)? else {
            return Ok(());
        };

        let decision = match ask_controller(&cfg, &candidate) {
            Ok(d) => d,
            Err(err) => {
                tracing::warn!(?err, "controller unreachable; declining");
                Decision::Decline
            }
        };

        let target = match decision {
            Decision::Decline => {
                eprintln!("# decline");
                continue;
            }
            Decision::Accept { target } => target,
        };

        let sentinel = inflight::write_sentinel(
            &cfg.inflight_dir,
            &Sentinel {
                pid: std::process::id(),
                drv_path: candidate.drv_path.clone(),
                admitted_at_ms: now_ms_u64(),
                predicted_ms: 0,
            },
        )?;
        let _guard = SentinelGuard { path: sentinel };

        let result = delegate_remote_build(&cfg, &settings, &candidate, &target, &mut stdin);
        let status = match &result {
            Ok(()) => BuildStatus::Success,
            Err(_) => BuildStatus::Failure,
        };
        let _ = report_admission_finish(&cfg, &candidate.drv_path, status);
        return result;
    }
}

fn ask_controller(cfg: &HookConfig, candidate: &HookCandidate) -> io::Result<Decision> {
    let mut stream = UnixStream::connect(&cfg.controller_socket)?;
    perform_handshake_sync(&mut stream)?;
    let body = DecideCandidate {
        drv_path: candidate.drv_path.clone(),
        system: candidate.needed_system.clone(),
        required_features: candidate.required_features.clone(),
        hook_pid: std::process::id(),
    };
    write_frame_sync(&mut stream, &Frame::with_body(op::DECIDE_CANDIDATE, &body)?)?;
    let reply = read_frame_sync(&mut stream)?;
    if reply.op_id != op::DECISION {
        return Err(io::Error::other(format!(
            "expected DECISION reply, got op_id {}",
            reply.op_id
        )));
    }
    reply.decode_body()
}

fn report_admission_finish(
    cfg: &HookConfig,
    drv_path: &str,
    status: BuildStatus,
) -> io::Result<()> {
    let mut stream = UnixStream::connect(&cfg.controller_socket)?;
    perform_handshake_sync(&mut stream)?;
    let body = AdmissionFinish {
        drv_path: drv_path.to_string(),
        status,
    };
    write_frame_sync(&mut stream, &Frame::with_body(op::ADMISSION_FINISH, &body)?)?;
    Ok(())
}

struct SentinelGuard {
    path: PathBuf,
}

impl Drop for SentinelGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
