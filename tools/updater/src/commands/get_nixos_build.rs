use anyhow::{Context, Result};
use clap::Args;

#[derive(Args, Debug)]
pub struct GetNixosBuildArgs {
    #[arg(long, default_value = "nixos-unstable")]
    channel: String,
    
    #[arg(long, help = "Output format", default_value = "plain")]
    format: OutputFormat,
}

#[derive(Debug, Clone, clap::ValueEnum)]
enum OutputFormat {
    Plain,
    Json,
}

pub async fn run(args: GetNixosBuildArgs) -> Result<()> {
    let commit_hash = get_latest_commit(&args.channel).await?;
    
    match args.format {
        OutputFormat::Plain => println!("{}", commit_hash),
        OutputFormat::Json => println!(r#"{{"commit": "{}"}}"#, commit_hash),
    }
    
    Ok(())
}

pub async fn get_latest_commit(channel: &str) -> Result<String> {
    let url = format!(
        "https://api.github.com/repos/NixOS/nixpkgs/commits?sha={}",
        channel
    );
    
    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .header("User-Agent", "nixos-updater")
        .send()
        .await
        .context("Failed to fetch from GitHub API")?;
    
    let commits: Vec<serde_json::Value> = response
        .json()
        .await
        .context("Failed to parse GitHub API response")?;
    
    commits
        .first()
        .and_then(|commit| commit["sha"].as_str())
        .map(String::from)
        .context("No commits found for channel")
}