//! Delegate an accepted candidate to `nix __build-remote`.
//!
//! Ported from `old-src/src/hook/delegate.rs` and adapted to use the
//! controller-supplied [`AcceptTarget`] builder line.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::process::{ChildStderr, Command, Stdio};
use std::thread;

use crate::protocol::ops::AcceptTarget;

use super::candidate::{write_hook_candidate, write_hook_settings, HookCandidate};
use super::HookConfig;
use crate::nix_protocol::{read_nix_strings, write_nix_strings};

pub fn delegate_remote_build<R: Read>(
    cfg: &HookConfig,
    settings: &[(String, String)],
    candidate: &HookCandidate,
    target: &AcceptTarget,
    parent_stdin: &mut R,
) -> io::Result<()> {
    let mut child = Command::new(&cfg.nix_bin)
        .arg("__build-remote")
        .arg(&cfg.verbosity)
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .stdout(Stdio::inherit())
        .spawn()?;

    {
        let child_stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "missing child stdin"))?;
        write_hook_settings(child_stdin, settings, &target.builder_line)?;
        write_hook_candidate(child_stdin, candidate)?;
        child_stdin.flush()?;
    }

    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "missing child stderr"))?;
    let accepted = proxy_child_until_decision(stderr)?;
    if !accepted {
        let _ = child.stdin.take();
        let _ = child.wait();
        return Ok(());
    }

    let inputs = read_nix_strings(parent_stdin)?;
    let wanted_outputs = read_nix_strings(parent_stdin)?;
    if let Some(child_stdin) = child.stdin.as_mut() {
        write_nix_strings(child_stdin, &inputs)?;
        write_nix_strings(child_stdin, &wanted_outputs)?;
        child_stdin.flush()?;
    }
    let _ = child.stdin.take();

    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "delegated nix __build-remote exited with {status}"
        )))
    }
}

fn proxy_child_until_decision(stderr: ChildStderr) -> io::Result<bool> {
    let mut reader = BufReader::new(stderr);
    let mut accepted = false;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Ok(false);
        }
        eprint!("{line}");
        let trimmed = line.trim_end();
        if trimmed == "# accept" {
            let mut store_uri = String::new();
            let n = reader.read_line(&mut store_uri)?;
            if n == 0 {
                return Ok(false);
            }
            eprint!("{store_uri}");
            accepted = true;
            break;
        }
        if trimmed == "# decline" || trimmed == "# decline-permanently" || trimmed == "# postpone" {
            break;
        }
    }

    if accepted {
        thread::spawn(move || {
            let mut reader = reader;
            let mut stderr = io::stderr();
            let _ = io::copy(&mut reader, &mut stderr);
        });
    }
    Ok(accepted)
}
