use anyhow::{Context, Result};
use std::process::Command;
use tracing::info;

pub fn restart(unit: &str) -> Result<()> {
    run("restart", unit)
}

pub fn stop(unit: &str) -> Result<()> {
    run("stop", unit)
}

pub fn start(unit: &str) -> Result<()> {
    run("start", unit)
}

fn run(action: &str, unit: &str) -> Result<()> {
    info!(%action, %unit, "systemctl {action} {unit}");
    let output = Command::new("systemctl")
        .arg(action)
        .arg(unit)
        .output()
        .with_context(|| format!("systemctl {action} {unit}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("systemctl {action} {unit} failed: {stderr}");
    }
    Ok(())
}
