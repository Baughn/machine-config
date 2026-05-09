mod api;
mod cli;
mod config;
mod daemon;
mod hook;
mod nix_protocol;
mod persistence;
mod scheduler;
mod telemetry;
mod util;

#[cfg(test)]
mod test_support;

use std::io;

use crate::api::client::send_event;
use crate::api::types::telemetry_json;
use crate::cli::{serve_config, Cli, CliCommand};
use crate::daemon::serve;
use crate::hook::run_hook;
use crate::telemetry::read_telemetry;

use clap::Parser;

fn main() {
    if let Err(err) = real_main() {
        eprintln!("nix-build-balancer: {err}");
        std::process::exit(1);
    }
}

fn real_main() -> io::Result<()> {
    match Cli::parse().command {
        Some(CliCommand::Serve(args)) => serve(serve_config(args, true)?),
        Some(CliCommand::Event(args)) => send_event(args.into()),
        Some(CliCommand::Hook(args)) => run_hook(args.try_into()?),
        Some(CliCommand::Telemetry(args)) => {
            let cfg = serve_config(args, false)?;
            println!("{}", telemetry_json(&read_telemetry(&cfg.host)?));
            Ok(())
        }
        None => Ok(()),
    }
}
