use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

use nbb::hook::{run_hook, HookConfig};

#[derive(Parser, Debug)]
#[command(name = "nbb-hook", about = "Nix build-hook talking to nbb-controller")]
struct Args {
    /// Path to the controller's Unix socket.
    #[arg(long, default_value = "/run/nbb/decide.sock")]
    controller_socket: PathBuf,

    /// Tmpfs directory where the hook writes its sentinel.
    #[arg(long, default_value = "/run/nbb/inflight")]
    inflight_dir: PathBuf,

    /// Path to the `nix` binary used for `nix __build-remote`.
    #[arg(long, default_value = "/run/current-system/sw/bin/nix")]
    nix_bin: PathBuf,

    /// Verbosity flag passed to `nix __build-remote`. Nix typically invokes
    /// the hook with this value as the second argument; the systemd module
    /// can pin it.
    #[arg(long, default_value = "0")]
    verbosity: String,

    /// Verbosity passed positionally by Nix when invoking the hook. Captured
    /// here so clap's positional argument doesn't choke; overrides `--verbosity`.
    positional_verbosity: Option<String>,
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

    let verbosity = args.positional_verbosity.unwrap_or(args.verbosity);
    let config = HookConfig {
        controller_socket: args.controller_socket,
        inflight_dir: args.inflight_dir,
        nix_bin: args.nix_bin,
        verbosity,
    };

    match run_hook(config) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("nbb-hook: {err}");
            ExitCode::FAILURE
        }
    }
}
