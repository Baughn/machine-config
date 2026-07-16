//! Build, diff, and deploy the machines in this flake — one evaluation,
//! a reboot-worthiness verdict per machine, remotes first, local host last.
//!
//! Design: docs/nix-deploy-design.md in the repository root.

mod activate;
mod build;
mod diff;
mod log_model;
mod manifest;
mod target;
mod ui;

use std::io::{IsTerminal, Write};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::Parser;

use activate::Mode;
use manifest::Machine;

#[derive(Debug, Parser)]
#[command(version, about = "Build and deploy the machines in deploy.toml")]
struct Cli {
    /// Machines to deploy (default: all, in manifest order).
    machines: Vec<String>,

    /// Force this activation mode everywhere, skipping the interactive
    /// prompt. Never reboots anything.
    #[arg(long, value_enum)]
    mode: Option<CliMode>,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum CliMode {
    Switch,
    Boot,
}

impl From<CliMode> for Mode {
    fn from(mode: CliMode) -> Self {
        match mode {
            CliMode::Switch => Mode::Switch,
            CliMode::Boot => Mode::Boot,
        }
    }
}

/// What to do with one machine after seeing its verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Switch,
    Boot,
    BootReboot,
    Exit,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let cwd = std::env::current_dir().context("getting the working directory")?;
    let root = manifest::find_repo_root(&cwd)?;
    // `nix build .#...` below needs the flake as the working directory.
    std::env::set_current_dir(&root).with_context(|| format!("entering {}", root.display()))?;

    let manifest = manifest::load(&root)?;
    let machines = select_machines(&manifest.machines, &cli.machines)?;
    let reboot_timeout = Duration::from_secs(manifest.settings.reboot_timeout_secs);

    let names: Vec<&str> = machines.iter().map(|m| m.name.as_str()).collect();
    eprintln!("building {} (single evaluation) ...", names.join(", "));
    let toplevels = build::build_toplevels(&machines)?;

    for machine in &machines {
        let new = toplevels[&machine.name].as_str();
        eprintln!("\n=== {} ({}) ===", machine.name, machine.target);

        let old = diff::current_system(&machine.target)?;
        if old == new {
            eprintln!("already up to date");
            continue;
        }

        let verdict = diff::assess(&machine.target, &old, new)?;
        diff::show_nvd_diff(&old, new);

        let action = decide(machine, &verdict, cli.mode)?;
        if action == Action::Exit {
            bail!(
                "aborted at {}; already-deployed machines stay deployed",
                machine.name
            );
        }

        activate::copy_closure(&machine.target, new)?;
        match action {
            Action::Switch => activate::activate(&machine.target, new, Mode::Switch)?,
            Action::Boot => {
                activate::activate(&machine.target, new, Mode::Boot)?;
                eprintln!(
                    "{}: boot entry set; reboot it when convenient",
                    machine.name
                );
            }
            Action::BootReboot => {
                activate::activate(&machine.target, new, Mode::Boot)?;
                if machine.target.is_local() {
                    eprintln!("rebooting {} now; goodbye", machine.name);
                    machine
                        .target
                        .run_streamed(&["systemctl", "reboot"], true)?;
                    return Ok(());
                }
                activate::reboot_and_wait(&machine.target, new, reboot_timeout)?;
            }
            Action::Exit => unreachable!("handled above"),
        }
    }

    eprintln!("\nall machines deployed");
    Ok(())
}

/// Resolve the requested machine names against the manifest, preserving
/// deploy order. No names selects everything.
fn select_machines(all: &[Machine], requested: &[String]) -> Result<Vec<Machine>> {
    if requested.is_empty() {
        return Ok(all.to_vec());
    }
    for name in requested {
        if !all.iter().any(|m| &m.name == name) {
            let known: Vec<&str> = all.iter().map(|m| m.name.as_str()).collect();
            bail!(
                "unknown machine {name:?}; deploy.toml knows: {}",
                known.join(", ")
            );
        }
    }
    Ok(all
        .iter()
        .filter(|m| requested.contains(&m.name))
        .cloned()
        .collect())
}

/// Pick the action for one machine: forced by --mode, defaulted when
/// nothing is reboot-worthy, otherwise prompted.
fn decide(machine: &Machine, verdict: &diff::Verdict, forced: Option<CliMode>) -> Result<Action> {
    if verdict.reboot_recommended() {
        eprintln!("{}: reboot recommended", machine.name);
        for reason in &verdict.reasons {
            eprintln!("  {reason}");
        }
    } else {
        eprintln!("{}: no reboot-worthy changes", machine.name);
    }

    if let Some(mode) = forced {
        if matches!(mode, CliMode::Switch) && verdict.reboot_recommended() {
            eprintln!("warning: switching anyway (--mode switch)");
        }
        return Ok(match Mode::from(mode) {
            Mode::Switch => Action::Switch,
            Mode::Boot => Action::Boot,
        });
    }
    if !verdict.reboot_recommended() {
        eprintln!("switching");
        return Ok(Action::Switch);
    }
    prompt(&machine.name)
}

fn prompt(machine: &str) -> Result<Action> {
    if !std::io::stdin().is_terminal() {
        bail!("{machine} needs a reboot decision but stdin is not a terminal; use --mode");
    }
    loop {
        eprint!("{machine}: [s]witch anyway / [b]oot only / [r] boot + reboot now / [e]xit? ");
        std::io::stderr().flush().ok();
        let mut line = String::new();
        if std::io::stdin()
            .read_line(&mut line)
            .context("reading the prompt answer")?
            == 0
        {
            bail!("stdin closed at the {machine} prompt");
        }
        match line.trim().to_lowercase().as_str() {
            "s" | "switch" => return Ok(Action::Switch),
            "b" | "boot" => return Ok(Action::Boot),
            "r" | "reboot" => return Ok(Action::BootReboot),
            "e" | "exit" | "q" | "quit" => return Ok(Action::Exit),
            other => eprintln!("unrecognized answer {other:?}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::target::Target;

    fn machines() -> Vec<Machine> {
        vec![
            Machine {
                name: "tsugumi".into(),
                target: Target::Ssh {
                    dest: "svein@tsugumi.local".into(),
                    sudo: true,
                },
            },
            Machine {
                name: "saya".into(),
                target: Target::Local { sudo: true },
            },
        ]
    }

    #[test]
    fn empty_selection_takes_all_in_order() {
        let picked = select_machines(&machines(), &[]).unwrap();
        let names: Vec<&str> = picked.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["tsugumi", "saya"]);
    }

    #[test]
    fn selection_preserves_manifest_order() {
        let picked =
            select_machines(&machines(), &["saya".to_string(), "tsugumi".to_string()]).unwrap();
        let names: Vec<&str> = picked.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["tsugumi", "saya"]);
    }

    #[test]
    fn unknown_machine_is_an_error() {
        let err = select_machines(&machines(), &["v4".to_string()]).unwrap_err();
        assert!(err.to_string().contains("unknown machine"));
    }
}
