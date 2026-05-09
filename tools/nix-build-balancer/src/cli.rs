use clap::{Args, Parser, Subcommand};
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use crate::api::types::BuildEvent;
use crate::config::{
    Config, Mode, DEFAULT_MAX_SAMPLES_PER_PNAME, DEFAULT_REMOTE_BUILDER, DEFAULT_REMOTE_HOST,
    DEFAULT_REMOTE_STORE_URI, DEFAULT_STALE_START_MS,
};
use crate::hook::HookConfig;
use crate::util::{hostname_fallback, invalid, now_ms};

#[derive(Parser)]
#[command(name = "nix-build-balancer", arg_required_else_help = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<CliCommand>,
}

#[derive(Subcommand)]
pub enum CliCommand {
    Serve(ServeArgs),
    Event(EventArgs),
    Hook(HookArgs),
    Telemetry(ServeArgs),
}

#[derive(Args)]
pub struct ServeArgs {
    #[arg(long, value_enum, default_value = "agent")]
    pub mode: Mode,
    #[arg(long, default_value_t = hostname_fallback())]
    pub host: String,
    #[arg(long, default_value = "/var/lib/nix-build-balancer")]
    pub data_dir: PathBuf,
    #[arg(long)]
    pub unix_socket: Option<PathBuf>,
    #[arg(long)]
    pub listen: Option<String>,
    #[arg(long)]
    pub remote: Vec<String>,
    #[arg(long, default_value_t = 1_000)]
    pub poll_interval_ms: u64,
    #[arg(long, default_value_t = DEFAULT_MAX_SAMPLES_PER_PNAME)]
    pub max_samples_per_pname: usize,
    #[arg(long, default_value_t = DEFAULT_STALE_START_MS)]
    pub stale_start_ms: u128,
    #[arg(long)]
    pub once: bool,
}

#[derive(Args)]
pub struct EventArgs {
    #[arg(long)]
    pub endpoint: String,
    #[arg(long)]
    pub kind: String,
    #[arg(long)]
    pub drv_path: String,
    #[arg(long, default_value = "")]
    pub out_paths: String,
    #[arg(long, default_value = "unknown")]
    pub status: String,
    #[arg(long, default_value_t = hostname_fallback())]
    pub host: String,
}

#[derive(Args)]
pub struct HookArgs {
    #[arg(long)]
    pub endpoint: String,
    #[arg(long, default_value_t = hostname_fallback())]
    pub host: String,
    #[arg(long, default_value = DEFAULT_REMOTE_HOST)]
    pub remote_host: String,
    #[arg(long, default_value = DEFAULT_REMOTE_STORE_URI)]
    pub remote_store_uri: String,
    #[arg(long, default_value = DEFAULT_REMOTE_BUILDER)]
    pub remote_builder: String,
    #[arg(long, default_value = "nix")]
    pub nix_bin: String,
    #[arg(value_name = "VERBOSITY", allow_hyphen_values = true, trailing_var_arg = true, num_args = 1..)]
    pub rest: Vec<String>,
}

pub fn serve_config(args: ServeArgs, require_listener: bool) -> io::Result<Config> {
    let cfg = Config {
        mode: args.mode,
        host: args.host,
        data_dir: args.data_dir,
        unix_socket: args.unix_socket,
        listen: args.listen,
        remote: args.remote,
        poll_interval: Duration::from_millis(args.poll_interval_ms),
        max_samples_per_pname: args.max_samples_per_pname,
        stale_start_ms: args.stale_start_ms,
        once: args.once,
    };

    if require_listener && cfg.unix_socket.is_none() && cfg.listen.is_none() && !cfg.once {
        return invalid("serve needs --unix-socket or --listen");
    }

    Ok(cfg)
}

impl From<EventArgs> for (String, BuildEvent) {
    fn from(args: EventArgs) -> Self {
        (
            args.endpoint,
            BuildEvent {
                kind: args.kind,
                drv_path: args.drv_path,
                out_paths: args.out_paths,
                status: args.status,
                host: args.host,
                timestamp_ms: now_ms(),
            },
        )
    }
}

impl TryFrom<HookArgs> for HookConfig {
    type Error = io::Error;

    fn try_from(args: HookArgs) -> io::Result<Self> {
        let Some(verbosity) = args.rest.first().cloned() else {
            return invalid("verbosity is required");
        };
        Ok(Self {
            endpoint: args.endpoint,
            host: args.host,
            remote_host: args.remote_host,
            remote_store_uri: args.remote_store_uri,
            remote_builder: args.remote_builder,
            nix_bin: args.nix_bin,
            verbosity,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clap_parses_serve_defaults_and_validation() {
        let command = Cli::try_parse_from(["nbb", "serve", "--once"])
            .unwrap()
            .command
            .unwrap();
        let CliCommand::Serve(args) = command else {
            panic!("expected serve command");
        };
        let cfg = serve_config(args, true).unwrap();

        assert!(matches!(cfg.mode, Mode::Agent));
        assert_eq!(cfg.data_dir, PathBuf::from("/var/lib/nix-build-balancer"));
        assert_eq!(cfg.poll_interval, Duration::from_secs(1));
        assert!(cfg.once);

        let command = Cli::try_parse_from(["nbb", "serve"])
            .unwrap()
            .command
            .unwrap();
        let CliCommand::Serve(args) = command else {
            panic!("expected serve command");
        };
        assert!(serve_config(args, true).is_err());
    }

    #[test]
    fn clap_parses_event_and_hook_args() {
        let command = Cli::try_parse_from([
            "nbb",
            "event",
            "--endpoint",
            "unix:/tmp/nbb.sock",
            "--kind",
            "start",
            "--drv-path",
            "/nix/store/hash-kwin-6.6.3.drv",
        ])
        .unwrap()
        .command
        .unwrap();
        let CliCommand::Event(args) = command else {
            panic!("expected event command");
        };
        let (endpoint, event): (String, BuildEvent) = args.into();
        assert_eq!(endpoint, "unix:/tmp/nbb.sock");
        assert_eq!(event.kind, "start");
        assert_eq!(event.status, "unknown");

        let command = Cli::try_parse_from([
            "nbb",
            "hook",
            "--endpoint",
            "unix:/tmp/nbb.sock",
            "--remote-host",
            "builder",
            "--",
            "-vv",
        ])
        .unwrap()
        .command
        .unwrap();
        let CliCommand::Hook(args) = command else {
            panic!("expected hook command");
        };
        let cfg = HookConfig::try_from(args).unwrap();
        assert_eq!(cfg.endpoint, "unix:/tmp/nbb.sock");
        assert_eq!(cfg.remote_host, "builder");
        assert_eq!(cfg.verbosity, "-vv");
    }
}
