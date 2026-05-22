//! `nbb-hook` — the Nix build-hook process.
//!
//! Invoked by Nix's `build-hook` setting on the controller host. For each
//! `try` candidate received from Nix on stdin, the hook asks the local
//! controller (over a Unix socket) whether to delegate. On `Decline` the
//! hook writes `# decline` to stderr and loops. On `Accept` it spawns
//! `nix __build-remote` with the controller-supplied builder line and
//! proxies the protocol through.
//!
//! Invariant: **every candidate is answered with exactly one directive on
//! stderr before the hook exits or moves to the next candidate.** A missing
//! directive crashes the Nix daemon with "unexpected EOF reading a line".
//! Per-candidate emission is enforced by [`guard::DirectiveGuard`], which
//! emits `# decline` on Drop unless the code explicitly emitted something.
//!
//! Sentinel lifecycle (SPEC §"Build observation lifecycle" item 4): the
//! hook writes `/run/nbb/inflight/<drv_hash>` after accept and unlinks it
//! on every exit path via a `Drop` guard. The controller's watchdog sweeps
//! these to retire admissions for crashed hooks.

pub mod candidate;
pub mod delegate;
pub mod guard;

use std::io;
use std::io::Read;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use crate::inflight::{self, Sentinel};
use crate::protocol::frame::{read_frame_sync, write_frame_sync, Frame};
use crate::protocol::handshake::perform_handshake_sync;
use crate::protocol::ops::{op, AdmissionFinish, BuildStatus, DecideCandidate, Decision};
use crate::util::now_ms_u64;

use candidate::{read_hook_candidate, read_hook_settings, HookCandidate};
use delegate::{delegate_remote_build, DelegateOutcome};
use guard::{DeclineKind, DirectiveGuard};

#[derive(Clone, Debug)]
pub struct HookConfig {
    pub controller_socket: PathBuf,
    pub inflight_dir: PathBuf,
    pub nix_bin: PathBuf,
    pub verbosity: String,
}

enum CandidateOutcome {
    /// Declined this candidate; continue reading the next `try`.
    Declined,
    /// Built (success or failure). The hook exits after one accepted build,
    /// matching Nix's one-process-per-derivation hook model.
    Finished(io::Result<()>),
}

pub fn run_hook(cfg: HookConfig) -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    let settings = read_hook_settings(&mut stdin)?;

    loop {
        let candidate = match read_hook_candidate(&mut stdin) {
            Ok(Some(c)) => c,
            Ok(None) => return Ok(()),
            Err(err) => {
                // Mid-`try` parse failure: Nix may already be waiting for a
                // directive. Emit decline-permanently so it stops asking us.
                tracing::warn!(?err, "parser failure reading try; decline-permanently");
                let mut guard = DirectiveGuard::new();
                guard.emit_decline(DeclineKind::DeclinePermanently);
                return Err(err);
            }
        };

        let mut guard = DirectiveGuard::new();
        let outcome = handle_candidate(&cfg, &settings, &candidate, &mut stdin, &mut guard);
        // Force directive emission before continuing, so the `# decline`
        // fallback (if any) lands before we read the next candidate.
        drop(guard);

        match outcome {
            CandidateOutcome::Declined => continue,
            CandidateOutcome::Finished(result) => return result,
        }
    }
}

fn handle_candidate<R: Read>(
    cfg: &HookConfig,
    settings: &[(String, String)],
    candidate: &HookCandidate,
    stdin: &mut R,
    guard: &mut DirectiveGuard,
) -> CandidateOutcome {
    let decision = match ask_controller(cfg, candidate) {
        Ok(d) => d,
        Err(err) => {
            tracing::warn!(?err, "controller unreachable; declining");
            Decision::Decline
        }
    };

    let target = match decision {
        Decision::Decline => {
            guard.decline();
            return CandidateOutcome::Declined;
        }
        Decision::Accept { target } => target,
    };

    let sentinel_path = match inflight::write_sentinel(
        &cfg.inflight_dir,
        &Sentinel {
            pid: std::process::id(),
            drv_path: candidate.drv_path.clone(),
            admitted_at_ms: now_ms_u64(),
            predicted_ms: 0,
        },
    ) {
        Ok(p) => p,
        Err(err) => {
            tracing::warn!(?err, "sentinel write failed; declining");
            guard.decline();
            return CandidateOutcome::Declined;
        }
    };
    let _sentinel_guard = SentinelGuard {
        path: sentinel_path,
    };

    let outcome = delegate_remote_build(cfg, settings, candidate, &target, stdin, guard);
    let (status, candidate_outcome) = match outcome {
        DelegateOutcome::Built => (BuildStatus::Success, CandidateOutcome::Finished(Ok(()))),
        DelegateOutcome::BuildFailed => (
            BuildStatus::Failure,
            CandidateOutcome::Finished(Err(io::Error::other(
                "delegated nix __build-remote failed",
            ))),
        ),
        DelegateOutcome::Declined => (BuildStatus::Cancelled, CandidateOutcome::Declined),
    };
    let _ = report_admission_finish(cfg, &candidate.drv_path, status);
    candidate_outcome
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
