//! Closure copy, profile update, activation, and the remote reboot dance.

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

/// Copy the closure of `path` to a remote target. No-op for local targets.
pub fn copy_closure(target: &Target, path: &str) -> Result<()> {
    let Some(dest) = target.ssh_dest() else {
        return Ok(());
    };
    let store_uri = format!("ssh://{dest}");
    let local = Target::Local { sudo: false };
    local
        .run_streamed(&["nix", "copy", "--to", &store_uri, path], false)
        .with_context(|| format!("copying {path} to {dest}"))
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
