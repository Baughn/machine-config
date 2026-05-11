use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

use nbb::agent::{run, AgentConfig};
use nbb::telemetry;
use nbb::util::hostname_fallback;

#[derive(Parser, Debug)]
#[command(name = "nbb-agent", about = "nix-build-balancer agent")]
struct Args {
    /// Print one telemetry snapshot and exit. Replaces the old `telemetry`
    /// CLI.
    #[arg(long)]
    once: bool,

    /// TCP bind address for the controller's polling connection.
    #[arg(long, default_value = "0.0.0.0:8765")]
    bind: SocketAddr,

    /// Spool directory `nbb-event` writes into.
    #[arg(long, default_value = "/var/lib/nbb/spool")]
    spool_dir: PathBuf,

    /// Hostname reported in `AGENT_HELLO`.
    #[arg(long)]
    hostname: Option<String>,

    /// Nix system identifier reported in `AGENT_HELLO`.
    #[arg(long, default_value = "x86_64-linux")]
    system: String,

    /// Local build capacity reported in `AGENT_HELLO` (parallel builds the
    /// controller may queue against this host).
    #[arg(long, default_value_t = 1)]
    capacity: u32,
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

    if args.once {
        match telemetry::sample() {
            Ok(t) => {
                println!("mem_available_kb={}", t.mem_available_kb);
                match t.psi_memory_some_avg10 {
                    Some(v) => println!("psi_memory_some_avg10={v}"),
                    None => println!("psi_memory_some_avg10=none"),
                }
                println!("nix_slots_active={}", t.nix_slots_active);
                println!("sampled_at_ms={}", t.sampled_at_ms);
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("telemetry sample failed: {err}");
                ExitCode::FAILURE
            }
        }
    } else {
        match run_async(args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("nbb-agent: {err}");
                ExitCode::FAILURE
            }
        }
    }
}

fn run_async(args: Args) -> io::Result<()> {
    let hostname = args.hostname.unwrap_or_else(hostname_fallback);
    let config = AgentConfig {
        bind_addr: args.bind,
        spool_dir: args.spool_dir,
        hostname,
        system: args.system,
        capacity: args.capacity,
    };
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(run(config))
}
