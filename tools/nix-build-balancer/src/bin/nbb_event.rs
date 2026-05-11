//! `nbb-event` — one-shot CLI invoked by Nix's `pre-build-hook` and
//! `post-build-hook`. Spec demands this binary be intentionally tiny: it
//! must never block a Nix build, never retry, and never fail noisily.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

use nbb::protocol::ops::{BuildStatus, SpoolEvent};
use nbb::spool::write_event;
use nbb::util::{hostname_fallback, now_ms_u64, pname_from_drv};

#[derive(Parser, Debug)]
#[command(
    name = "nbb-event",
    about = "Submit a build event to the local nbb-agent spool"
)]
struct Args {
    /// `start` (from `pre-build-hook`) or `finish` (from `post-build-hook`).
    #[arg(long, value_parser = ["start", "finish"])]
    kind: String,

    /// Derivation path. For `pre-build-hook` this is `$1`; for
    /// `post-build-hook` this is `$DRV_PATH`.
    #[arg(long)]
    drv_path: String,

    /// Hostname recorded in the spool entry.
    #[arg(long)]
    host: Option<String>,

    /// Spool directory.
    #[arg(long, default_value = "/var/lib/nbb/spool")]
    spool_dir: PathBuf,

    /// `success` / `failure` (only used with `--kind finish`).
    #[arg(long, default_value = "success")]
    status: String,

    /// Whitespace- or newline-separated `OUT_PATHS` (only used with
    /// `--kind finish`).
    #[arg(long, default_value = "")]
    out_paths: String,
}

fn main() -> ExitCode {
    // Per spec: never fail a Nix build. Any error returns ExitCode::SUCCESS
    // after logging to stderr.
    let args = Args::parse();
    if let Err(err) = submit(args) {
        eprintln!("nbb-event: {err}");
    }
    ExitCode::SUCCESS
}

fn submit(args: Args) -> std::io::Result<()> {
    let host = args.host.unwrap_or_else(hostname_fallback);
    let pname = pname_from_drv(&args.drv_path);
    let ts_ms = now_ms_u64();

    let event = match args.kind.as_str() {
        "start" => SpoolEvent::Start {
            drv_path: args.drv_path,
            pname,
            host,
            ts_ms,
        },
        "finish" => SpoolEvent::Finish {
            drv_path: args.drv_path,
            pname,
            host,
            ts_ms,
            status: parse_status(&args.status),
            out_paths: split_out_paths(&args.out_paths),
        },
        other => {
            return Err(std::io::Error::other(format!(
                "unknown --kind {other:?}; expected 'start' or 'finish'"
            )));
        }
    };

    write_event(&args.spool_dir, &event)?;
    Ok(())
}

fn parse_status(s: &str) -> BuildStatus {
    BuildStatus::parse(s).unwrap_or(BuildStatus::Failure)
}

fn split_out_paths(s: &str) -> Vec<String> {
    s.split_whitespace().map(|s| s.to_string()).collect()
}
