# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Repository Overview

This is a Rust CLI tool called `aniwatch` that manages anime file synchronization between directories. It automatically copies new anime files from a download directory to a watched directory, tracks what was copied, and can clean up old files after 4 weeks.

## Architecture

The project uses:
- `src/main.rs`: Main application with CLI commands and file tracking logic
- State persistence in `~/.config/aniwatch/state.json` to track copied files
- File system operations for copying and cleaning up anime files

## Common Commands

### Building and Running

```bash
# Build the project
cargo build

# Build in release mode
cargo build --release

# Run the project
cargo run

# Run with arguments
cargo run -- <args>
```

### Testing

```bash
# Run all tests
cargo test

# Run tests with output displayed
cargo test -- --nocapture

# Run a specific test
cargo test <test_name>
```

### Development

```bash
# Check code without building
cargo check

# Format code
cargo fmt

# Run linter
cargo clippy

# Update dependencies
cargo update
```

### Usage Commands

```bash
# Initialize the tool (must be run first)
cargo run -- init

# Sync new files from ~/Anime to ~/Sync/Watched
cargo run -- sync
cargo run -- sync --dry-run  # Preview what would be copied

# Clean up files older than 4 weeks
cargo run -- clean
cargo run -- clean --dry-run  # Preview what would be deleted

# Check status and pending deletions
cargo run -- status
```

## Dependencies

The project uses:
- `anyhow` (1.0.98): For simplified error handling
- `clap` (4.5.39): For command-line argument parsing with derive feature enabled
- `chrono` (0.4.41): For date/time handling and 4-week calculations
- `serde` & `serde_json`: For state file serialization
- `dirs` (6.0.0): For finding home and config directories
- `walkdir` (2.5.0): For recursive directory traversal