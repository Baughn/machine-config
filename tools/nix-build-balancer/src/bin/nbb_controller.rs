use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::Parser;

use nbb::controller::{run, ControllerConfig};
use nbb::estimator;
use nbb::scheduler::{SchedulerPolicy, Target};

#[derive(Parser, Debug)]
#[command(name = "nbb-controller", about = "nix-build-balancer controller")]
struct Args {
    /// Nix system this controller routes for.
    #[arg(long, default_value = "x86_64-linux")]
    system: String,

    /// Persistent state directory. Holds `state.db`.
    #[arg(long, default_value = "/var/lib/nbb")]
    data_dir: PathBuf,

    /// Tmpfs directory under which `nbb-hook` writes per-build sentinels.
    #[arg(long, default_value = "/run/nbb/inflight")]
    inflight_dir: PathBuf,

    /// Unix socket where `nbb-hook` connects.
    #[arg(long, default_value = "/run/nbb/decide.sock")]
    hook_socket: PathBuf,

    /// One or more targets, each `name=tcp_addr,capacity,store_uri,builder_line[,is_local][,speed=X]`.
    /// Repeat the flag for additional targets. Commas inside the
    /// `builder_line` need quoting from the shell.
    #[arg(long = "target", value_parser = parse_target)]
    targets: Vec<Target>,

    #[arg(long, default_value_t = 1000)]
    poll_interval_ms: u64,

    #[arg(long, default_value_t = 1_000_000)]
    min_remote_mem_available_kb: u64,

    #[arg(long, default_value_t = 60_000)]
    unknown_p95_ms: u64,

    #[arg(long, default_value_t = 200)]
    max_samples_per_pname: u32,

    /// EWMA smoothing factor α for the per-pname duration estimator.
    /// Must be in (0, 1]. Half-life in observations is ln(0.5)/ln(1−α);
    /// the default 0.2 → ≈3.1 obs.
    #[arg(long, default_value_t = estimator::ALPHA_DEFAULT, value_parser = parse_alpha)]
    ewma_alpha: f64,

    /// Standard-normal quantile multiplier z for the estimator's
    /// upper-tail read-out. Default 1.645 ≈ Φ⁻¹(0.95). Use 1.96 for
    /// ≈Φ⁻¹(0.975) if you want extra conservatism at the cost of more
    /// over-estimation.
    #[arg(long, default_value_t = estimator::Z_P95, value_parser = parse_z)]
    ewma_z: f64,
}

fn parse_alpha(s: &str) -> Result<f64, String> {
    let v: f64 = s.parse().map_err(|e| format!("bad ewma-alpha: {e}"))?;
    if !v.is_finite() || v <= 0.0 || v > 1.0 {
        return Err(format!("ewma-alpha must be in (0, 1], got {v}"));
    }
    Ok(v)
}

fn parse_z(s: &str) -> Result<f64, String> {
    let v: f64 = s.parse().map_err(|e| format!("bad ewma-z: {e}"))?;
    if !v.is_finite() || v < 0.0 {
        return Err(format!("ewma-z must be ≥ 0 and finite, got {v}"));
    }
    Ok(v)
}

fn parse_target(s: &str) -> Result<Target, String> {
    // Expected: name=tcp_addr|capacity|store_uri|builder_line[|is_local][|speed=X]
    // Pipe-separated to avoid clashing with commas in builder_line.
    let (name, rest) = s
        .split_once('=')
        .ok_or_else(|| "target must be name=...".to_string())?;
    let parts: Vec<&str> = rest.split('|').collect();
    if parts.len() < 4 {
        return Err("target needs tcp_addr|capacity|store_uri|builder_line".to_string());
    }
    let tcp_addr: SocketAddr = parts[0].parse().map_err(|e| format!("bad tcp_addr: {e}"))?;
    let capacity: u32 = parts[1].parse().map_err(|e| format!("bad capacity: {e}"))?;
    let store_uri = parts[2].to_string();
    let builder_line = parts[3].to_string();
    let mut is_controller_host = false;
    let mut speed_multiplier: f64 = 1.0;
    for extra in &parts[4..] {
        if *extra == "is_local" {
            is_controller_host = true;
        } else if let Some(v) = extra.strip_prefix("speed=") {
            speed_multiplier = v.parse().map_err(|e| format!("bad speed: {e}"))?;
        } else {
            return Err(format!("unknown target option: {extra}"));
        }
    }
    Ok(Target {
        name: name.to_string(),
        tcp_addr,
        store_uri,
        builder_line,
        capacity,
        speed_multiplier,
        is_controller_host,
    })
}

fn main() -> ExitCode {
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("NBB_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    if args.targets.is_empty() {
        eprintln!("nbb-controller: at least one --target required");
        return ExitCode::FAILURE;
    }

    let config = ControllerConfig {
        system: args.system,
        data_dir: args.data_dir,
        inflight_dir: args.inflight_dir,
        hook_socket: args.hook_socket,
        targets: args.targets,
        poll_interval: Duration::from_millis(args.poll_interval_ms),
        policy: SchedulerPolicy {
            min_remote_mem_available_kb: args.min_remote_mem_available_kb,
            unknown_p95_ms: args.unknown_p95_ms,
        },
        max_samples_per_pname: args.max_samples_per_pname,
        ewma_alpha: args.ewma_alpha,
        ewma_z: args.ewma_z,
    };

    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(err) => {
            eprintln!("nbb-controller: tokio runtime: {err}");
            return ExitCode::FAILURE;
        }
    };

    match rt.block_on(run(config)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("nbb-controller: {err}");
            ExitCode::FAILURE
        }
    }
}
