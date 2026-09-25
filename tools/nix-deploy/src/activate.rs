//! Closure copy, profile update, activation, and the remote reboot dance.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

use crate::target::Target;

pub const SYSTEM_PROFILE: &str = "/nix/var/nix/profiles/system";

/// How the new system is activated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Switch,
    Boot,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Switch => "switch",
            Mode::Boot => "boot",
        }
    }
}

/// Parallel ssh connections for `nix copy`. With one, the remote daemon
/// (hashing + writing a single path at a time) caps out around 370 MiB/s.
const COPY_CONNECTIONS: u32 = 8;

/// Copy the closure of `path` to a remote target. No-op for local targets.
///
/// Nix multiplexes all store connections over one ssh ControlMaster, whose
/// single-threaded crypto then becomes the bottleneck; disable that so each
/// connection gets its own ssh process. Any user-set `NIX_SSHOPTS` goes
/// first, since ssh takes the first value given for an option.
pub fn copy_closure(target: &Target, path: &str) -> Result<()> {
    let Some(dest) = target.ssh_dest() else {
        return Ok(());
    };
    let store_uri = format!("ssh://{dest}?max-connections={COPY_CONNECTIONS}");
    let mut sshopts = std::env::var("NIX_SSHOPTS").unwrap_or_default();
    sshopts.push_str(" -oControlMaster=no -oControlPath=none");
    let status = Command::new("nix")
        .args(["copy", "--to", &store_uri, path])
        .env("NIX_SSHOPTS", sshopts.trim_start())
        .stdin(Stdio::null())
        .status()
        .with_context(|| format!("spawning nix copy to {dest}"))?;
    if !status.success() {
        bail!("copying {path} to {dest} failed ({status})");
    }
    Ok(())
}

/// Point the system profile at `path` and run switch-to-configuration.
pub fn activate(target: &Target, path: &str, mode: Mode) -> Result<()> {
    target
        .run_streamed(
            &["nix-env", "--profile", SYSTEM_PROFILE, "--set", path],
            true,
        )
        .with_context(|| format!("setting the system profile on {target}"))?;
    let stc = format!("{path}/bin/switch-to-configuration");
    target
        .run_streamed(&[&stc, mode.as_str()], true)
        .with_context(|| format!("switch-to-configuration {} on {target}", mode.as_str()))
}

/// Reboot a remote target and wait until it is back on the new system.
///
/// Verifies the boot id changed (so an early ssh success against the old
/// boot doesn't count), the system reached `running` (or `degraded`, with a
/// warning), and `/run/current-system` is `expected_system`.
///
/// # Errors
///
/// Errors on timeout, a failed boot state, or an unexpected running system.
pub fn reboot_and_wait(target: &Target, expected_system: &str, timeout: Duration) -> Result<()> {
    const BOOT_ID: &str = "/proc/sys/kernel/random/boot_id";
    let old_boot_id = target
        .run_capture(&["cat", BOOT_ID], false)
        .context("reading the boot id before reboot")?;

    eprintln!("rebooting {target} ...");
    // The connection usually dies mid-command; any error here is expected.
    let _ = target.run_capture_unchecked(&["systemctl", "reboot"], true);

    let deadline = Instant::now() + timeout;
    loop {
        std::thread::sleep(Duration::from_secs(5));
        if Instant::now() > deadline {
            bail!("{target} did not come back within {}s", timeout.as_secs());
        }
        match target.run_capture_unchecked(&["cat", BOOT_ID], false) {
            Ok((true, boot_id)) if boot_id != old_boot_id => break,
            _ => continue,
        }
    }

    let (_, state) = target
        .run_capture_unchecked(&["systemctl", "is-system-running", "--wait"], false)
        .context("querying the post-reboot system state")?;
    match state.as_str() {
        "running" => {}
        "degraded" => {
            eprintln!("warning: {target} is degraded after reboot; failed units:");
            let _ = target.run_streamed(&["systemctl", "--failed", "--no-pager"], false);
        }
        other => bail!("{target} is in state {other:?} after reboot"),
    }

    let current = target
        .run_capture(&["readlink", "-f", "/run/current-system"], false)
        .context("verifying the running system after reboot")?;
    if current != expected_system {
        bail!("{target} booted {current}, expected {expected_system}");
    }
    eprintln!("{target} is back on the new system");
    Ok(())
}
