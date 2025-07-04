use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::Serialize;
use std::process::Command;


mod parser;
use parser::{parse_jj_jsonl_output, FileChangeType};

// Hardcoded path to the agenix-decrypted webhook URL
const WEBHOOK_PATH: &str = "/run/agenix/erisia-webhook.url";

#[derive(Parser)]
#[command(author, version, about = "Send Discord notifications")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Check for file changes and notify if patterns match
    Update {
        /// File patterns to check (comma-separated)
        #[arg(short, long)]
        patterns: String,

        /// Custom message (optional)
        #[arg(short, long)]
        message: Option<String>,

        /// Revision filter for jj log
        #[arg(short, long, default_value = "immutable_heads()..latest(ancestors(@) & ~empty() & ~description(exact:\"\"))")]
        revision: String,

        /// Show what would be sent without sending
        #[arg(long)]
        dry_run: bool,
    },
    /// Send an ad-hoc message
    Send {
        /// Message to send
        message: String,

        /// Show what would be sent without sending
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Serialize)]
struct DiscordWebhook {
    content: String,
    embeds: Vec<Embed>,
}

#[derive(Serialize)]
struct Embed {
    title: String,
    description: String,
    color: u32,
    fields: Vec<Field>,
}

#[derive(Serialize)]
struct Field {
    name: String,
    value: String,
    inline: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    
    match cli.command {
        Commands::Update { patterns, message, revision, dry_run } => {
            handle_update(patterns, message, revision, dry_run)
        }
        Commands::Send { message, dry_run } => {
            handle_send(message, dry_run)
        }
    }
}

fn handle_update(patterns: String, message: Option<String>, revision: String, dry_run: bool) -> Result<()> {
    // NOTE: Using /home/svein/.cargo/bin/jj (version 0.31) for json() support
    // TODO: Switch to system jj when NixOS packages jj 0.31+
    
    // Get commit data as JSONL
    let json_output = Command::new("/home/svein/.cargo/bin/jj")
        .args(&["log", "--no-graph", "-r", &revision, "-T", "json(self) ++ \"\\n\""])
        .output()
        .context("Failed to execute jj log for commit data")?;
    
    if !json_output.status.success() {
        anyhow::bail!("jj log (json) failed: {}", String::from_utf8_lossy(&json_output.stderr));
    }
    
    // Get file changes separately using diff.summary()
    let diff_output = Command::new("/home/svein/.cargo/bin/jj")
        .args(&["log", "--no-graph", "-r", &revision, "-T", "diff.summary()"])
        .output()
        .context("Failed to execute jj log for file changes")?;
    
    if !diff_output.status.success() {
        anyhow::bail!("jj log (diff) failed: {}", String::from_utf8_lossy(&diff_output.stderr));
    }
    
    let jsonl_output = String::from_utf8_lossy(&json_output.stdout);
    let diff_summary = String::from_utf8_lossy(&diff_output.stdout);
    
    // Parse patterns
    let patterns: Vec<&str> = patterns.split(',').map(|s| s.trim()).collect();
    
    // Parse jj log output using the new JSONL parser
    let parsed = parse_jj_jsonl_output(&jsonl_output, &diff_summary)
        .map_err(|e| anyhow::anyhow!("Failed to parse jj log output: {}", e))?;
    
    // Find matching files
    let mut matching_files = Vec::new();
    for file_change in &parsed.file_changes {
        // Check if file matches any pattern
        for pattern in &patterns {
            let file_matches = if pattern.contains('*') {
                // Simple glob matching
                let pattern_parts: Vec<&str> = pattern.split('*').collect();
                let mut matches = true;
                let mut pos = 0;
                
                for (i, part) in pattern_parts.iter().enumerate() {
                    if part.is_empty() {
                        continue;
                    }
                    
                    if i == 0 && !file_change.path.starts_with(part) {
                        matches = false;
                        break;
                    } else if i == pattern_parts.len() - 1 && !file_change.path.ends_with(part) {
                        matches = false;
                        break;
                    } else if i > 0 {
                        if let Some(idx) = file_change.path[pos..].find(part) {
                            pos += idx + part.len();
                        } else {
                            matches = false;
                            break;
                        }
                    }
                }
                matches
            } else {
                file_change.path == *pattern
            };
            
            if file_matches {
                let change_type_str = match file_change.change_type {
                    FileChangeType::Added => "A",
                    FileChangeType::Modified => "M",
                    FileChangeType::Deleted => "D",
                    FileChangeType::Renamed => "R",
                };
                matching_files.push(format!("{} {}", change_type_str, file_change.path));
                break;
            }
        }
    }
    
    if matching_files.is_empty() {
        println!("No files matching patterns: {}", patterns.join(","));
        return Ok(());
    }
    
    // Build Discord message with multi-commit support
    let (hash, email, timestamp) = if let Some(latest_commit) = parsed.commits.first() {
        (latest_commit.hash.clone(), latest_commit.author.clone(), latest_commit.timestamp.clone())
    } else {
        ("unknown".to_string(), "unknown".to_string(), "unknown".to_string())
    };
    
    // Create commit summary
    let commit_descriptions: Vec<String> = parsed.commits
        .iter()
        .filter_map(|c| {
            if c.description.is_empty() || c.description.contains("(no description set)") {
                None
            } else {
                Some(c.description.clone())
            }
        })
        .collect();
    
    let commit_summary = if commit_descriptions.is_empty() {
        "No commit messages".to_string()
    } else if commit_descriptions.len() == 1 {
        commit_descriptions[0].clone()
    } else {
        format!("{} commits:\n{}", commit_descriptions.len(), commit_descriptions.join("\n"))
    };
    
    let title = if let Some(custom_msg) = message {
        custom_msg
    } else if commit_descriptions.len() == 1 {
        format!("NixOS config update: {}", commit_descriptions[0])
    } else {
        format!("NixOS config update: {} commits", commit_descriptions.len())
    };
    
    let webhook = DiscordWebhook {
        content: format!("🔧 **{}**", title),
        embeds: vec![Embed {
            title: "Commit Details".to_string(),
            description: commit_summary,
            color: 0x2ECC71, // Green
            fields: vec![
                Field {
                    name: "Author".to_string(),
                    value: email,
                    inline: true,
                },
                Field {
                    name: "Latest Commit".to_string(),
                    value: format!("`{}`", &hash[..8.min(hash.len())]),
                    inline: true,
                },
                Field {
                    name: "Timestamp".to_string(),
                    value: timestamp,
                    inline: true,
                },
                Field {
                    name: "Changed Files".to_string(),
                    value: if matching_files.len() > 10 {
                        format!("{}\n... and {} more files", 
                               matching_files[..10].join("\n"), 
                               matching_files.len() - 10)
                    } else {
                        matching_files.join("\n")
                    },
                    inline: false,
                },
            ],
        }],
    };
    
    send_webhook(webhook, dry_run)
}

fn handle_send(message: String, dry_run: bool) -> Result<()> {
    let webhook = DiscordWebhook {
        content: message,
        embeds: vec![],
    };
    
    send_webhook(webhook, dry_run)
}

fn send_webhook(webhook: DiscordWebhook, dry_run: bool) -> Result<()> {
    let json = serde_json::to_string(&webhook)?;
    
    if dry_run {
        println!("Would send to Discord:");
        println!("{}", json);
        return Ok(());
    }
    
    // Read webhook URL
    let webhook_url = std::fs::read_to_string(WEBHOOK_PATH.trim())
        .context("Failed to read webhook URL")?
        .trim()
        .to_string();
    
    // Send webhook
    let client = reqwest::blocking::Client::new();
    let response = client
        .post(&webhook_url)
        .header("Content-Type", "application/json")
        .body(json)
        .send()
        .context("Failed to send webhook")?;
    
    if !response.status().is_success() {
        anyhow::bail!("Discord webhook failed: {} - {}", response.status(), response.text()?);
    }
    
    println!("Discord notification sent successfully");
    Ok(())
}