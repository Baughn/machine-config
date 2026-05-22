//! Delegate an accepted candidate to `nix __build-remote`.
//!
//! Every code path here either tells the [`DirectiveGuard`] to emit a
//! directive, or leaves it to fall through to the `Drop` fallback. The
//! function never propagates an error before the directive is decided.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::process::{ChildStderr, Command, Stdio};
use std::thread;

use crate::protocol::ops::AcceptTarget;

use super::candidate::{write_hook_candidate, write_hook_settings, HookCandidate};
use super::guard::{DeclineKind, DirectiveGuard};
use super::HookConfig;
use crate::nix_protocol::{read_nix_strings, write_nix_strings};

/// What happened to the candidate after delegation.
pub enum DelegateOutcome {
    /// `nix __build-remote` ran to completion successfully.
    Built,
    /// `nix __build-remote` ran but exited non-zero.
    BuildFailed,
    /// Declined before commit — child crashed, errored, or said `# decline*`.
    Declined,
}

pub fn delegate_remote_build<R: Read>(
    cfg: &HookConfig,
    settings: &[(String, String)],
    candidate: &HookCandidate,
    target: &AcceptTarget,
    parent_stdin: &mut R,
    guard: &mut DirectiveGuard,
) -> DelegateOutcome {
    let mut child = match Command::new(&cfg.nix_bin)
        .arg("__build-remote")
        .arg(&cfg.verbosity)
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .stdout(Stdio::inherit())
        .spawn()
    {
        Ok(c) => c,
        Err(err) => {
            tracing::warn!(?err, "spawn nix __build-remote failed; declining");
            guard.decline();
            return DelegateOutcome::Declined;
        }
    };

    if let Err(err) = write_child_prefix(&mut child, settings, candidate, target) {
        tracing::warn!(?err, "writing prefix to child stdin failed; declining");
        guard.decline();
        let _ = child.kill();
        let _ = child.wait();
        return DelegateOutcome::Declined;
    }

    let stderr = match child.stderr.take() {
        Some(s) => s,
        None => {
            tracing::warn!("child stderr unavailable; declining");
            guard.decline();
            let _ = child.kill();
            let _ = child.wait();
            return DelegateOutcome::Declined;
        }
    };

    let directive = match read_child_directive(stderr) {
        Ok(d) => d,
        Err(err) => {
            tracing::warn!(?err, "reading child directive failed; declining");
            guard.decline();
            let _ = child.kill();
            let _ = child.wait();
            return DelegateOutcome::Declined;
        }
    };

    match directive {
        ChildDirective::Decline { kind, reader } => {
            guard.emit_decline(kind);
            spawn_stderr_drain(reader);
            let _ = child.stdin.take();
            let _ = child.wait();
            DelegateOutcome::Declined
        }
        ChildDirective::None => {
            guard.decline();
            let _ = child.stdin.take();
            let _ = child.wait();
            DelegateOutcome::Declined
        }
        ChildDirective::Accept { store_uri, reader } => {
            guard.accept(&store_uri);
            spawn_stderr_drain(reader);
            run_accepted_build(child, parent_stdin)
        }
    }
}

fn write_child_prefix(
    child: &mut std::process::Child,
    settings: &[(String, String)],
    candidate: &HookCandidate,
    target: &AcceptTarget,
) -> io::Result<()> {
    let child_stdin = child
        .stdin
        .as_mut()
        .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "missing child stdin"))?;
    write_hook_settings(child_stdin, settings, &target.builder_line)?;
    write_hook_candidate(child_stdin, candidate)?;
    child_stdin.flush()
}

fn run_accepted_build<R: Read>(
    mut child: std::process::Child,
    parent_stdin: &mut R,
) -> DelegateOutcome {
    // We have committed by emitting `# accept`. From here on, any failure is
    // a build failure, not a protocol-violating early exit.
    let inputs = match read_nix_strings(parent_stdin) {
        Ok(v) => v,
        Err(err) => {
            tracing::warn!(?err, "reading inputs from parent stdin failed");
            let _ = child.kill();
            let _ = child.wait();
            return DelegateOutcome::BuildFailed;
        }
    };
    let wanted_outputs = match read_nix_strings(parent_stdin) {
        Ok(v) => v,
        Err(err) => {
            tracing::warn!(?err, "reading wanted_outputs failed");
            let _ = child.kill();
            let _ = child.wait();
            return DelegateOutcome::BuildFailed;
        }
    };

    if let Some(child_stdin) = child.stdin.as_mut() {
        let write_result = (|| -> io::Result<()> {
            write_nix_strings(child_stdin, &inputs)?;
            write_nix_strings(child_stdin, &wanted_outputs)?;
            child_stdin.flush()
        })();
        if let Err(err) = write_result {
            tracing::warn!(?err, "writing inputs/wanted_outputs to child failed");
            let _ = child.kill();
            let _ = child.wait();
            return DelegateOutcome::BuildFailed;
        }
    }
    let _ = child.stdin.take();

    match child.wait() {
        Ok(status) if status.success() => DelegateOutcome::Built,
        Ok(status) => {
            tracing::warn!(?status, "delegated nix __build-remote exited non-zero");
            DelegateOutcome::BuildFailed
        }
        Err(err) => {
            tracing::warn!(?err, "wait on delegated child failed");
            DelegateOutcome::BuildFailed
        }
    }
}

fn spawn_stderr_drain(reader: BufReader<ChildStderr>) {
    thread::spawn(move || {
        let mut reader = reader;
        let mut stderr = io::stderr();
        let _ = io::copy(&mut reader, &mut stderr);
    });
}

enum ChildDirective {
    Accept {
        store_uri: String,
        reader: BufReader<ChildStderr>,
    },
    Decline {
        kind: DeclineKind,
        reader: BufReader<ChildStderr>,
    },
    /// Child closed stderr before emitting any directive.
    None,
}

fn read_child_directive(stderr: ChildStderr) -> io::Result<ChildDirective> {
    let mut reader = BufReader::new(stderr);
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Ok(ChildDirective::None);
        }
        let trimmed = line.trim_end();
        match trimmed {
            "# accept" => {
                let mut store_uri = String::new();
                let n = reader.read_line(&mut store_uri)?;
                if n == 0 {
                    return Ok(ChildDirective::None);
                }
                let uri = store_uri.trim_end().to_string();
                return Ok(ChildDirective::Accept {
                    store_uri: uri,
                    reader,
                });
            }
            "# decline" => {
                return Ok(ChildDirective::Decline {
                    kind: DeclineKind::Decline,
                    reader,
                });
            }
            "# decline-permanently" => {
                return Ok(ChildDirective::Decline {
                    kind: DeclineKind::DeclinePermanently,
                    reader,
                });
            }
            "# postpone" => {
                return Ok(ChildDirective::Decline {
                    kind: DeclineKind::Postpone,
                    reader,
                });
            }
            _ => {
                // Non-directive log line. Forward to our stderr as build log.
                eprint!("{line}");
            }
        }
    }
}
