use anyhow::{Context, Result};
use clap::Parser;
use colored::*;
use redis::{ConnectionInfo, TypedCommands};
use serde::{Deserialize, Serialize};
use std::fs;
use std::process::{Command as StdCommand, Stdio};
use tokio::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Parser)]
#[command(author, version, about = "Cached nix flake check using Redis")]
struct Args {
    /// TTL for cache entries in seconds
    #[arg(long, default_value = "2592000")]
    ttl: u64,
    
    /// Disable cache and run check directly
    #[arg(long)]
    no_cache: bool,
    
    /// Redis server address
    #[arg(long, default_value = "10.171.0.1")]
    redis_addr: String,
    
    /// Path to Redis password file
    #[arg(long, default_value = "/run/agenix/redis-nixcheck-password")]
    redis_password_file: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct CacheEntry {
    exit_code: i32,
    stdout: String,
    stderr: String,
    timestamp: u64,
}

struct JujutsuState {
    original_commit: String,
    temp_commit_created: bool,
}

impl JujutsuState {
    fn new() -> Result<Self> {
        // Get the original commit ID before creating temp commit
        let output = StdCommand::new("jj")
            .args(&["log", "-r", "@", "--no-graph", "-T", "self.commit_id()"])
            .output()
            .context("Failed to get current commit ID")?;
        
        if !output.status.success() {
            anyhow::bail!("jj log command failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        
        let original_commit = String::from_utf8(output.stdout)
            .context("Invalid UTF-8 in commit ID")?
            .trim()
            .to_string();
        
        Ok(Self {
            original_commit,
            temp_commit_created: false,
        })
    }
    
    fn create_temp_commit(&mut self) -> Result<()> {
        let output = StdCommand::new("jj")
            .args(&["new"])
            .output()
            .context("Failed to create temporary commit")?;
        
        if !output.status.success() {
            anyhow::bail!("jj new command failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        
        self.temp_commit_created = true;
        println!("{}", "Created temporary commit".dimmed());
        Ok(())
    }
    
    fn get_cache_key(&self) -> Result<String> {
        if self.temp_commit_created {
            anyhow::bail!("Must get cache key before creating temp commit");
        }
        
        let output = StdCommand::new("jj")
            .args(&["log", "-r", "@", "--no-graph", "-T", "self.commit_id()"])
            .output()
            .context("Failed to get commit hash for cache key")?;
        
        if !output.status.success() {
            anyhow::bail!("jj log command failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        
        let commit_hash = String::from_utf8(output.stdout)
            .context("Invalid UTF-8 in commit hash")?
            .trim()
            .to_string();
        
        Ok(format!("nix-check:{}", commit_hash))
    }
    
    fn has_uncommitted_changes(&self) -> Result<bool> {
        let output = StdCommand::new("jj")
            .args(&["diff"])
            .output()
            .context("Failed to check for uncommitted changes")?;
        
        if !output.status.success() {
            anyhow::bail!("jj diff command failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        
        Ok(!output.stdout.is_empty())
    }
    
    fn cleanup(&mut self) -> Result<()> {
        if !self.temp_commit_created {
            return Ok(());
        }
        
        // Verify no uncommitted changes before abandoning
        if self.has_uncommitted_changes()? {
            anyhow::bail!("Uncommitted changes detected - cannot safely abandon temporary commit");
        }
        
        let output = StdCommand::new("jj")
            .args(&["edit", "@-"])
            .output()
            .context("Failed to abandon temporary commit")?;
        
        if !output.status.success() {
            anyhow::bail!("jj abandon command failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        
        self.temp_commit_created = false;
        println!("{}", "Cleaned up temporary commit".dimmed());
        Ok(())
    }
}

impl Drop for JujutsuState {
    fn drop(&mut self) {
        if self.temp_commit_created {
            if let Err(e) = self.cleanup() {
                eprintln!("{}: Failed to cleanup temporary commit: {:#}", "Warning".yellow(), e);
            }
        }
    }
}

async fn get_redis_connection(args: &Args) -> Result<redis::Client> {
    let password = fs::read_to_string(&args.redis_password_file)
        .with_context(|| format!("Failed to read Redis password from {}", args.redis_password_file))?
        .trim()
        .to_string();
    
    let redis_ci = ConnectionInfo {
        addr: redis::ConnectionAddr::Tcp(args.redis_addr.clone(), 6379),
        redis: redis::RedisConnectionInfo { db: 0, username: Some("nixcheck".to_string()), password: Some(password), protocol: redis::ProtocolVersion::RESP3 },
    };
    let client = redis::Client::open(redis_ci)
        .context("Failed to create Redis client")?;
    
    
    Ok(client)
}

async fn get_cached_result(cache_key: &str, conn: &mut redis::Client) -> Result<Option<CacheEntry>> {
    let cached: Option<String> = conn.get(cache_key)
        .context("Failed to get value from Redis")?;
    
    match cached {
        Some(data) => {
            let entry: CacheEntry = serde_json::from_str(&data)
                .context("Failed to deserialize cache entry")?;
            Ok(Some(entry))
        }
        None => Ok(None)
    }
}

async fn store_cached_result(cache_key: &str, entry: &CacheEntry, ttl: u64, conn: &mut redis::Client) -> Result<()> {
    println!("Caching result: {}", if entry.exit_code == 0 {"success"} else {"failure"});
    let data = serde_json::to_string(entry)
        .context("Failed to serialize cache entry")?;
    
    let _: () = conn.set_ex(cache_key, data, ttl)
        .context("Failed to store result in Redis")?;
    
    Ok(())
}

async fn run_nix_flake_check() -> Result<CacheEntry> {
    println!("{}", "Running nix flake check...".cyan());
    
    let start = std::time::Instant::now();
    let cmd = Command::new("nix")
        .args(&["flake", "check"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to start nix flake check")?;
    
    let output = cmd.wait_with_output().await
        .context("Failed to wait for nix flake check completion")?;
    
    let duration = start.elapsed();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    let entry = CacheEntry {
        exit_code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        timestamp,
    };
    
    let status_msg = if entry.exit_code == 0 {
        format!("✓ nix flake check completed successfully in {:.2}s", duration.as_secs_f64()).green()
    } else {
        format!("✗ nix flake check failed with exit code {} in {:.2}s", entry.exit_code, duration.as_secs_f64()).red()
    };
    println!("{}", status_msg);
    
    Ok(entry)
}

fn print_output(entry: &CacheEntry, from_cache: bool) {
    if from_cache {
        let age = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() - entry.timestamp;
        println!("{} (cached {} seconds ago)", "Using cached result".green(), age);
    }
    
    if !entry.stdout.is_empty() {
        print!("{}", entry.stdout);
    }
    
    if !entry.stderr.is_empty() {
        eprint!("{}", entry.stderr);
    }
}

async fn run_main_logic(args: Args, jj_state: &mut JujutsuState) -> Result<i32> {
    
    // If cache is disabled, run directly
    if args.no_cache {
        let entry = run_nix_flake_check().await?;
        print_output(&entry, false);
        jj_state.cleanup()?;
        return Ok(entry.exit_code);
    }
    
    let cache_key = jj_state.get_cache_key()
        .context("Failed to generate cache key")?;
    
    
    // Try to get cached result
    let _cached_entry: Option<CacheEntry> = match get_redis_connection(&args).await {
        Ok(mut conn) => {
            match get_cached_result(&cache_key, &mut conn).await {
                Ok(cached) => {
                    if let Some(entry) = cached {
                        print_output(&entry, true);
                        jj_state.cleanup()?;
                        return Ok(entry.exit_code);
                    }
                    None
                }
                Err(e) => {
                    eprintln!("{}: Failed to get cached result: {:#}", "Warning".yellow(), e);
                    None
                }
            }
        }
        Err(e) => {
            eprintln!("{}: Redis connection failed, running without cache: {:#}", "Warning".yellow(), e);
            None
        }
    };
    
    // Cache miss - run the actual check
    println!("{}", "Cache miss - running nix flake check".yellow());

    // Create temporary commit for nix flake heck
    jj_state.create_temp_commit()
        .context("Failed to create temporary commit")?;

    let entry = run_nix_flake_check().await?;
    
    // Try to store result in cache
    if let Ok(mut conn) = get_redis_connection(&args).await {
        if let Err(e) = store_cached_result(&cache_key, &entry, args.ttl, &mut conn).await {
            eprintln!("{}: Failed to store result in cache: {:#}", "Warning".yellow(), e);
        } else {
            println!("{}", format!("Cached result for {} seconds", args.ttl).dimmed());
        }
    }
    
    print_output(&entry, false);
    jj_state.cleanup()?;
    Ok(entry.exit_code)
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    
    // Initialize Jujutsu state in main scope so cleanup is guaranteed
    let mut jj_state = JujutsuState::new()
        .context("Failed to initialize Jujutsu state")?;
    
    let exit_code = tokio::select! {
        result = run_main_logic(args, &mut jj_state) => {
            match result {
                Ok(code) => code,
                Err(e) => {
                    eprintln!("Error: {:#}", e);
                    1
                }
            }
        }
        _ = tokio::signal::ctrl_c() => {
            println!("\nReceived Ctrl+C, cleaning up...");
            130 // Standard exit code for SIGINT
        }
    };
    
    // Ensure cleanup always happens, regardless of exit path
    if let Err(e) = jj_state.cleanup() {
        eprintln!("{}: Failed to cleanup temporary commit: {:#}", "Warning".yellow(), e);
    }
    
    std::process::exit(exit_code);
}
