mod config;
mod detect;
mod firewall;
mod gpu;
mod rules;
mod service;

use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{info, warn};
use tracing_subscriber::{prelude::*, EnvFilter};

#[derive(Parser)]
#[command(version, about = "Per-game firewall and service manager")]
struct Cli {
    /// Path to the TOML config file.
    #[arg(long)]
    config: PathBuf,

    /// Run only the startup cleanup pass (remove stale firewall rules) and exit.
    #[arg(long)]
    cleanup_only: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    init_tracing();

    info!("game-watcher starting");

    match firewall::cleanup_stale() {
        Ok(n) if n > 0 => info!(removed = n, "startup cleanup removed stale rules"),
        Ok(_) => info!("startup cleanup: no stale rules"),
        Err(e) => warn!(error = %e, "startup cleanup failed; continuing"),
    }

    if cli.cleanup_only {
        return Ok(());
    }

    let cfg = config::Config::load(&cli.config).context("loading config")?;
    info!(
        games = cfg.games.len(),
        guards = cfg.gpu_guards.len(),
        "config loaded"
    );

    let poll_interval = Duration::from_millis(cfg.poll_interval_ms);
    let gpu_interval = Duration::from_millis(cfg.gpu_poll_interval_ms);

    let mut engine = rules::Engine::new(cfg).context("initialising rule engine")?;
    let mut active = detect::ActiveGames::new();

    // NVML init is best-effort — the daemon can still manage firewall rules
    // even if the GPU subsystem fails to come up.
    let mut gpu_monitor = match gpu::GpuMonitor::new() {
        Ok(g) => Some(g),
        Err(e) => {
            warn!(error = %e, "GPU monitor unavailable; guards will be disabled");
            None
        }
    };

    let shutdown = Arc::new(AtomicBool::new(false));
    install_signal_handlers(shutdown.clone())?;

    let mut next_gpu_poll = Instant::now();

    while !shutdown.load(Ordering::Relaxed) {
        let loop_start = Instant::now();

        let scanned = detect::scan_proc();
        let (started, stopped) = active.tick(scanned);
        for id in started {
            engine.on_game_start(id);
        }
        for id in stopped {
            engine.on_game_stop(id);
        }

        if let Some(ref mut gpu) = gpu_monitor {
            if loop_start >= next_gpu_poll {
                if let Err(e) = engine.poll_gpu(gpu) {
                    warn!(error = %e, "gpu poll failed");
                }
                next_gpu_poll = loop_start + gpu_interval;
            }
        }

        // Sleep in short chunks so SIGTERM wakes us promptly.
        let sleep_end = loop_start + poll_interval;
        while !shutdown.load(Ordering::Relaxed) {
            let now = Instant::now();
            if now >= sleep_end {
                break;
            }
            let remaining = sleep_end - now;
            std::thread::sleep(remaining.min(Duration::from_millis(200)));
        }
    }

    info!("shutdown signal received");
    engine.shutdown();
    info!("game-watcher exiting");
    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let registry = tracing_subscriber::registry().with(filter);
    match tracing_journald::layer() {
        Ok(journald) => {
            registry.with(journald).init();
        }
        Err(e) => {
            // Fallback to stderr when not running under systemd.
            eprintln!("journald unavailable ({e}); logging to stderr");
            registry.with(tracing_subscriber::fmt::layer()).init();
        }
    }
}

fn install_signal_handlers(flag: Arc<AtomicBool>) -> Result<()> {
    use signal_hook::consts::{SIGINT, SIGTERM};
    use signal_hook::iterator::Signals;

    let mut signals = Signals::new([SIGINT, SIGTERM]).context("registering signal handlers")?;
    std::thread::spawn(move || {
        for sig in signals.forever() {
            info!(%sig, "signal received");
            flag.store(true, Ordering::Relaxed);
            break;
        }
    });
    Ok(())
}

