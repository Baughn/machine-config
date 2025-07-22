use anyhow::{Context, Result};
use clap::Args;
use std::process::Command;
use std::path::PathBuf;
use tracing::{debug,info};

use super::get_nixos_build;

#[derive(Args, Debug)]
pub struct RebaseNixpkgsArgs {
    #[arg(long, default_value = "nixos-unstable")]
    channel: String,
    
    #[arg(long, help = "Path to nixpkgs repository")]
    nixpkgs_path: Option<PathBuf>,
}

pub async fn run(args: RebaseNixpkgsArgs) -> Result<()> {
    let nixpkgs_path = args.nixpkgs_path.unwrap_or_else(|| {
        let home = std::env::var("HOME").expect("HOME not set");
        PathBuf::from(home).join("dev/nixpkgs")
    });
    
    if !nixpkgs_path.exists() {
        anyhow::bail!("nixpkgs path does not exist: {}", nixpkgs_path.display());
    }
    
    debug!("Fetching latest commit for {}...", args.channel);
    let target_commit = get_nixos_build::get_latest_commit(&args.channel).await?;
    debug!("Target commit: {}", target_commit);
    
    debug!("Fetching latest changes in nixpkgs...");
    let output = Command::new("jj")
        .current_dir(&nixpkgs_path)
        .args(["git", "fetch"])
        .output()
        .context("Failed to run jj git fetch")?;
    
    if !output.status.success() {
        anyhow::bail!("jj git fetch failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    
    info!("Rebasing WIP commits onto {}...", target_commit);
    let output = Command::new("jj")
        .current_dir(&nixpkgs_path)
        .args(["rebase", "-r", "mutable()", "-d", &target_commit])
        .output()
        .context("Failed to run jj rebase")?;
    
    if !output.status.success() {
        anyhow::bail!("jj rebase failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    
    debug!("Successfully rebased onto {}", target_commit);

    let output = Command::new("jj")
        .current_dir(&nixpkgs_path)
        .args(["bookmark", "move", "master", "--to", &target_commit, "--allow-backwards"])
        .output()
        .context("Failed to move master bookmark")?;
    if !output.status.success() {
        anyhow::bail!("Failed to move master bookmark: {}", String::from_utf8_lossy(&output.stderr));
    }
    
    let output = Command::new("jj")
        .current_dir(&nixpkgs_path)
        .args(["log", "--limit", "15"])
        .output()
        .context("Failed to run jj log")?;
    
    info!("Current history:\n{}", String::from_utf8_lossy(&output.stdout));
    
    Ok(())
}
