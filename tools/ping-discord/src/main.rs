use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::Serialize;
use std::process::Command;

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
    // Get changed files from jj
    let output = Command::new("jj")
        .args(&["log", "-s", "-r", &revision])
        .output()
        .context("Failed to execute jj log")?;
    
    if !output.status.success() {
        anyhow::bail!("jj log failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    
    let jj_output = String::from_utf8_lossy(&output.stdout);
    
    // Parse patterns
    let patterns: Vec<&str> = patterns.split(',').map(|s| s.trim()).collect();
    
    // Parse jj log output
    let mut matching_files = Vec::new();
    let mut commit_info = None;
    let mut current_commit_message = String::new();
    let mut in_commit_message = false;
    
    for line in jj_output.lines() {
        if line.starts_with("○") || line.starts_with("@") || line.starts_with("◉") {
            // Parse commit header line: ○  qkqmkllk sveina@gmail.com 2025-07-04 00:48:49 git_head() fdbf7d3c
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 6 {
                let hash_short = parts[1];
                let email = parts[2];
                let date = parts[3];
                let time = parts[4];
                let hash_full = parts[parts.len() - 1]; // Last part is the full hash
                commit_info = Some((hash_full.to_string(), email.to_string(), format!("{} {}", date, time)));
                in_commit_message = true;
            }
        } else if (line.starts_with("│") || line.starts_with("~")) && in_commit_message {
            // This is the commit message line
            let msg = line.trim_start_matches(['│', '~']).trim();
            if !msg.is_empty() && current_commit_message.is_empty() {
                current_commit_message = msg.to_string();
                in_commit_message = false;
            }
        } else if line.trim().starts_with(|c: char| c == 'M' || c == 'A' || c == 'D' || c == 'R') {
            // Parse file change with change type
            let line_trimmed = line.trim();
            if line_trimmed.len() > 2 {
                let change_type = &line_trimmed[0..1];
                let file = &line_trimmed[2..]; // Skip the change type and space
                
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
                            
                            if i == 0 && !file.starts_with(part) {
                                matches = false;
                                break;
                            } else if i == pattern_parts.len() - 1 && !file.ends_with(part) {
                                matches = false;
                                break;
                            } else if i > 0 {
                                if let Some(idx) = file[pos..].find(part) {
                                    pos += idx + part.len();
                                } else {
                                    matches = false;
                                    break;
                                }
                            }
                        }
                        matches
                    } else {
                        file == *pattern
                    };
                    
                    if file_matches {
                        matching_files.push(format!("{} {}", change_type, file));
                        break;
                    }
                }
            }
        }
    }
    
    if matching_files.is_empty() {
        println!("No files matching patterns: {}", patterns.join(","));
        return Ok(());
    }
    
    // Build Discord message
    let (hash, email, timestamp) = commit_info.unwrap_or_else(|| {
        ("unknown".to_string(), "unknown".to_string(), "unknown".to_string())
    });
    
    let commit_msg = if current_commit_message.is_empty() {
        "No commit message".to_string()
    } else {
        current_commit_message
    };
    
    let title = if let Some(custom_msg) = message {
        custom_msg
    } else {
        format!("NixOS config update: {}", commit_msg)
    };
    
    let webhook = DiscordWebhook {
        content: format!("🔧 **{}**", title),
        embeds: vec![Embed {
            title: "Commit Details".to_string(),
            description: commit_msg.clone(),
            color: 0x2ECC71, // Green
            fields: vec![
                Field {
                    name: "Author".to_string(),
                    value: email,
                    inline: true,
                },
                Field {
                    name: "Commit".to_string(),
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