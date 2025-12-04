use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Local};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration as StdDuration;
use walkdir::WalkDir;

#[derive(Parser)]
#[command(name = "aniwatch")]
#[command(about = "Anime file synchronization tool", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Sync {
        #[arg(short, long)]
        dry_run: bool,
    },
    Clean {
        #[arg(short, long)]
        dry_run: bool,
    },
    Status,
    Init,
}

#[derive(Debug, Serialize, Deserialize)]
struct TrackedFile {
    source_path: PathBuf,
    dest_path: PathBuf,
    copied_at: DateTime<Local>,
    auto_copied: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct State {
    tracked_files: HashMap<String, TrackedFile>,
    initialized_at: DateTime<Local>,
    preexisting_files: HashSet<String>,
}

impl State {
    fn new() -> Self {
        Self {
            tracked_files: HashMap::new(),
            initialized_at: Local::now(),
            preexisting_files: HashSet::new(),
        }
    }

    fn load() -> Result<Option<Self>> {
        let state_path = get_state_path()?;
        if !state_path.exists() {
            return Ok(None);
        }
        
        let content = fs::read_to_string(&state_path)
            .with_context(|| format!("Failed to read state file: {:?}", state_path))?;
        
        let state: State = serde_json::from_str(&content)
            .with_context(|| "Failed to parse state file")?;
        
        Ok(Some(state))
    }

    fn save(&self) -> Result<()> {
        let state_path = get_state_path()?;
        let content = serde_json::to_string_pretty(self)
            .with_context(|| "Failed to serialize state")?;
        
        fs::write(&state_path, content)
            .with_context(|| format!("Failed to write state file: {:?}", state_path))?;
        
        Ok(())
    }
}

fn get_state_path() -> Result<PathBuf> {
    let config_dir = dirs::config_dir()
        .context("Failed to get config directory")?;
    
    let app_dir = config_dir.join("aniwatch");
    fs::create_dir_all(&app_dir)
        .with_context(|| format!("Failed to create config directory: {:?}", app_dir))?;
    
    Ok(app_dir.join("state.json"))
}

fn get_anime_dir() -> Result<PathBuf> {
    let home = dirs::home_dir()
        .context("Failed to get home directory")?;
    Ok(home.join("Anime"))
}

fn get_sync_dir() -> Result<PathBuf> {
    let home = dirs::home_dir()
        .context("Failed to get home directory")?;
    Ok(home.join("Sync").join("Watched"))
}

fn scan_existing_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    
    for entry in WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        files.push(entry.path().to_path_buf());
    }
    
    Ok(files)
}

fn is_file_quiescent(path: &Path, wait_time: StdDuration) -> Result<bool> {
    let metadata1 = fs::metadata(path)
        .with_context(|| format!("Failed to get metadata for {:?}", path))?;
    
    let size1 = metadata1.len();
    let modified1 = metadata1.modified()
        .with_context(|| format!("Failed to get modified time for {:?}", path))?;
    
    thread::sleep(wait_time);
    
    let metadata2 = fs::metadata(path)
        .with_context(|| format!("Failed to get metadata for {:?}", path))?;
    
    let size2 = metadata2.len();
    let modified2 = metadata2.modified()
        .with_context(|| format!("Failed to get modified time for {:?}", path))?;
    
    Ok(size1 == size2 && modified1 == modified2)
}

fn init_command() -> Result<()> {
    let state_path = get_state_path()?;
    
    if state_path.exists() {
        println!("State file already exists. Aborting initialization.");
        return Ok(());
    }
    
    let anime_dir = get_anime_dir()?;
    let sync_dir = get_sync_dir()?;
    
    println!("Initializing aniwatch...");
    println!("Anime directory: {:?}", anime_dir);
    println!("Sync directory: {:?}", sync_dir);
    
    let existing_anime_files = scan_existing_files(&anime_dir)?;
    let existing_sync_files = scan_existing_files(&sync_dir)?;
    
    println!("Found {} files in anime directory", existing_anime_files.len());
    println!("Found {} files in sync directory", existing_sync_files.len());
    
    let mut state = State::new();
    
    // Store filenames of all preexisting files
    for file in &existing_anime_files {
        if let Some(filename) = file.file_name().and_then(|n| n.to_str()) {
            state.preexisting_files.insert(filename.to_string());
        }
    }
    
    state.save()?;
    
    println!("Initialization complete. {} existing files will be ignored.", state.preexisting_files.len());
    Ok(())
}

fn sync_command(dry_run: bool) -> Result<()> {
    let mut state = match State::load()? {
        Some(s) => s,
        None => {
            println!("No state file found. Please run 'aniwatch init' first.");
            return Ok(());
        }
    };
    
    let anime_dir = get_anime_dir()?;
    let sync_dir = get_sync_dir()?;
    
    let current_anime_files = scan_existing_files(&anime_dir)?;
    let mut synced_count = 0;
    
    for file in current_anime_files {
        let file_name = file.file_name()
            .and_then(|n| n.to_str())
            .context("Invalid file name")?;
        
        // Skip if this file was preexisting
        if state.preexisting_files.contains(&file_name.to_string()) {
            continue;
        }
        
        // Skip if already tracked
        if state.tracked_files.contains_key(file_name) {
            continue;
        }
        
        let dest_path = sync_dir.join(file_name);
        
        if dest_path.exists() {
            println!("Skipping {:?}: already exists in sync directory", file_name);
            continue;
        }
        
        if dry_run {
            println!("Would copy: {:?} -> {:?}", file, dest_path);
            synced_count += 1;
        } else {
            // Wait until file is stable
            loop {
                if is_file_quiescent(&file, StdDuration::from_secs(2))? {
                    break;
                }
                println!("File {:?} is still being written, waiting...", file_name);
            }
            
            println!("Copying: {:?} -> {:?}", file, dest_path);
            fs::copy(&file, &dest_path)
                .with_context(|| format!("Failed to copy {:?} to {:?}", file, dest_path))?;
            
            state.tracked_files.insert(
                file_name.to_string(),
                TrackedFile {
                    source_path: file.clone(),
                    dest_path: dest_path.clone(),
                    copied_at: Local::now(),
                    auto_copied: true,
                }
            );
            synced_count += 1;
        }
    }
    
    if !dry_run {
        state.save()?;
    }
    
    println!("\nSync complete: {} files {}", synced_count, if dry_run { "would be synced" } else { "synced" });
    
    Ok(())
}

fn clean_command(dry_run: bool) -> Result<()> {
    let mut state = match State::load()? {
        Some(s) => s,
        None => {
            println!("No state file found. Please run 'aniwatch init' first.");
            return Ok(());
        }
    };
    
    let horizon = Local::now() - Duration::weeks(2);
    let mut files_to_remove = Vec::new();
    let mut cleaned_count = 0;
    
    for (filename, tracked_file) in &state.tracked_files {
        if !tracked_file.auto_copied {
            continue;
        }
        
        if tracked_file.copied_at < horizon {
            if tracked_file.dest_path.exists() {
                if dry_run {
                    println!("Would delete: {:?} (copied on {})", 
                        tracked_file.dest_path, 
                        tracked_file.copied_at.format("%Y-%m-%d"));
                } else {
                    println!("Deleting: {:?} (copied on {})", 
                        tracked_file.dest_path, 
                        tracked_file.copied_at.format("%Y-%m-%d"));
                    fs::remove_file(&tracked_file.dest_path)
                        .with_context(|| format!("Failed to delete {:?}", tracked_file.dest_path))?;
                }
                files_to_remove.push(filename.clone());
                cleaned_count += 1;
            }
        }
    }
    
    if !dry_run {
        for filename in files_to_remove {
            state.tracked_files.remove(&filename);
        }
        state.save()?;
    }
    
    println!("\nClean complete: {} files {}", cleaned_count, if dry_run { "would be deleted" } else { "deleted" });
    
    Ok(())
}

fn status_command() -> Result<()> {
    let state = match State::load()? {
        Some(s) => s,
        None => {
            println!("No state file found. Please run 'aniwatch init' first.");
            return Ok(());
        }
    };
    
    println!("Aniwatch Status");
    println!("===============");
    println!("Initialized: {}", state.initialized_at.format("%Y-%m-%d %H:%M:%S"));
    println!("Tracked files: {}", state.tracked_files.len());
    
    let auto_copied: Vec<_> = state.tracked_files.values()
        .filter(|f| f.auto_copied)
        .collect();
    
    println!("Auto-copied files: {}", auto_copied.len());
    
    let horizon = Local::now() - Duration::weeks(2);
    let pending_deletion: Vec<_> = auto_copied.iter()
        .filter(|f| f.copied_at < horizon && f.dest_path.exists())
        .collect();
    
    println!("Files pending deletion: {}", pending_deletion.len());
    
    if !pending_deletion.is_empty() {
        println!("\nFiles that will be deleted on next clean:");
        for file in pending_deletion {
            println!("  - {:?} (copied {})", 
                file.dest_path.file_name().unwrap_or_default(),
                file.copied_at.format("%Y-%m-%d"));
        }
    }
    
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    
    match cli.command {
        Some(Commands::Init) => init_command(),
        Some(Commands::Sync { dry_run }) => sync_command(dry_run),
        Some(Commands::Clean { dry_run }) => clean_command(dry_run),
        Some(Commands::Status) => status_command(),
        None => {
            println!("Aniwatch - Anime file synchronization tool");
            println!("\nUse --help for usage information");
            Ok(())
        }
    }
}
