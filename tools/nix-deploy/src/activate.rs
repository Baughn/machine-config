//! Closure copy, profile update, activation, and the remote reboot dance.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

use crate::target::{run_command_streaming, Target};

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

fn build_copy_command(dest: &str, path: &str) -> Command {
    let store_uri = format!("ssh://{dest}?max-connections={COPY_CONNECTIONS}");
    let mut sshopts = std::env::var("NIX_SSHOPTS").unwrap_or_default();
    sshopts.push_str(" -oControlMaster=no -oControlPath=none");
    let mut cmd = Command::new("nix");
    cmd.args(["copy", "--to", &store_uri, path])
        .env("NIX_SSHOPTS", sshopts.trim_start())
        .stdin(Stdio::null());
    cmd
}

/// Copy the closure of `path` to a remote target. No-op for local targets.
///
/// Nix multiplexes all store connections over one ssh ControlMaster, whose
/// single-threaded crypto then becomes the bottleneck; disable that so each
/// connection gets its own ssh process. Any user-set `NIX_SSHOPTS` goes
/// first, since ssh takes the first value given for an option.
///
/// # Errors
///
/// Errors if `nix copy` fails to spawn or exits non-zero.
pub fn copy_closure(target: &Target, path: &str) -> Result<()> {
    let Some(dest) = target.ssh_dest() else {
        return Ok(());
    };
    let status = build_copy_command(dest, path)
        .status()
        .with_context(|| format!("spawning nix copy to {dest}"))?;
    if !status.success() {
        bail!("copying {path} to {dest} failed ({status})");
    }
    Ok(())
}

/// Copy the closure of `path` to a remote target while streaming combined
/// stdout and stderr lines to `on_line`. No-op for local targets.
///
/// # Errors
///
/// Errors if `nix copy` fails to spawn or exits non-zero.
pub fn copy_closure_streaming<F>(target: &Target, path: &str, mut on_line: F) -> Result<()>
where
    F: FnMut(String),
{
    let Some(dest) = target.ssh_dest() else {
        return Ok(());
    };
    on_line(format!("copying closure to {dest} ..."));
    let mut cmd = build_copy_command(dest, path);
    run_command_streaming(&mut cmd, &format!("nix copy to {dest}"), on_line)
}

/// Point the system profile at `path` and run switch-to-configuration.
///
/// # Errors
///
/// Errors if setting the profile or running `switch-to-configuration` fails.
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

/// Point the system profile at `path` and run `switch-to-configuration`,
/// streaming output lines to `on_line` instead of inheriting the terminal.
///
/// # Errors
///
/// Errors if setting the profile or running `switch-to-configuration` fails.
pub fn activate_streaming<F>(target: &Target, path: &str, mode: Mode, mut on_line: F) -> Result<()>
where
    F: FnMut(String),
{
    on_line(format!("setting system profile on {target} ..."));
    target
        .run_streaming(
            &["nix-env", "--profile", SYSTEM_PROFILE, "--set", path],
            true,
            &mut on_line,
        )
        .with_context(|| format!("setting the system profile on {target}"))?;
    on_line(format!(
        "running switch-to-configuration {} on {target} ...",
        mode.as_str()
    ));
    let stc = format!("{path}/bin/switch-to-configuration");
    target
        .run_streaming(&[&stc, mode.as_str()], true, &mut on_line)
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
    reboot_and_wait_streaming(target, expected_system, timeout, |line| {
        eprintln!("{line}");
    })
}

/// Reboot a remote target and wait until it is back on the new system,
/// reporting progress and any failed units via `on_line`.
///
/// # Errors
///
/// Errors on timeout, a failed boot state, or an unexpected running system.
pub fn reboot_and_wait_streaming<F>(
    target: &Target,
    expected_system: &str,
    timeout: Duration,
    mut on_line: F,
) -> Result<()>
where
    F: FnMut(String),
{
    const BOOT_ID: &str = "/proc/sys/kernel/random/boot_id";
    let old_boot_id = target
        .run_capture(&["cat", BOOT_ID], false)
        .context("reading the boot id before reboot")?;

    on_line(format!("rebooting {target} ..."));
    // The connection usually dies mid-command; any error here is expected.
    let _ = target.run_capture_unchecked(&["systemctl", "reboot"], true);

    let deadline = Instant::now() + timeout;
    let started = Instant::now();
    loop {
        std::thread::sleep(Duration::from_secs(5));
        if Instant::now() > deadline {
            bail!("{target} did not come back within {}s", timeout.as_secs());
        }
        on_line(format!(
            "waiting for {target} to return ({}s elapsed) ...",
            started.elapsed().as_secs()
        ));
        match target.run_capture_unchecked(&["cat", BOOT_ID], false) {
            Ok((true, boot_id)) if boot_id != old_boot_id => break,
            _ => continue,
        }
    }

    on_line(format!(
        "{target} booted; waiting for systemd to settle ..."
    ));
    let (_, state) = target
        .run_capture_unchecked(&["systemctl", "is-system-running", "--wait"], false)
        .context("querying the post-reboot system state")?;
    match state.as_str() {
        "running" => {}
        "degraded" => {
            on_line(format!(
                "warning: {target} is degraded after reboot; failed units:"
            ));
            let _ = target.run_streaming(
                &["systemctl", "--failed", "--no-pager"],
                false,
                &mut on_line,
            );
        }
        other => bail!("{target} is in state {other:?} after reboot"),
    }

    let current = target
        .run_capture(&["readlink", "-f", "/run/current-system"], false)
        .context("verifying the running system after reboot")?;
    if current != expected_system {
        bail!("{target} booted {current}, expected {expected_system}");
    }
    on_line(format!("{target} is back on the new system"));
    Ok(())
}
