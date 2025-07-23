use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use nixos_updater::commands;

#[derive(Parser)]
#[command(name = "nixos-updater")]
#[command(about = "NixOS update workflow tools", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Get the latest nixos-unstable commit")]
    GetNixosBuild(commands::get_nixos_build::GetNixosBuildArgs),
    
    #[command(about = "Rebase nixpkgs WIP commits onto latest nixos-unstable")]
    RebaseNixpkgs(commands::rebase_nixpkgs::RebaseNixpkgsArgs),
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info"))
        )
        .init();
    
    let cli = Cli::parse();
    
    match cli.command {
        Commands::GetNixosBuild(args) => commands::get_nixos_build::run(args).await,
        Commands::RebaseNixpkgs(args) => commands::rebase_nixpkgs::run(args).await,
    }
}
