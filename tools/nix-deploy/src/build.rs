//! Single-evaluation build of every selected machine's system toplevel.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, IsTerminal, Read};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use anyhow::{bail, Context, Result};

use crate::log_model::Model;
use crate::manifest::Machine;
use crate::ui::Ui;

/// Build all toplevels in one `nix build` invocation (one evaluation) and
/// return machine name → out path.
///
/// # Errors
///
/// Errors if the build fails or an out path can't be matched to a machine.
pub fn build_toplevels(machines: &[Machine]) -> Result<BTreeMap<String, String>> {
    let installables: Vec<String> = machines
        .iter()
        .map(|m| {
            format!(
                ".#nixosConfigurations.{}.config.system.build.toplevel",
                m.name
            )
        })
        .collect();

    let use_ui = std::io::stderr().is_terminal();
    let mut cmd = Command::new("nix");
    cmd.args(["build", "--no-link", "--print-out-paths"]);
    if use_ui {
        cmd.args(["--log-format", "internal-json"]);
        cmd.stderr(Stdio::piped());
    }
    cmd.args(&installables);
    cmd.stdout(Stdio::piped()).stdin(Stdio::null());

    let mut child = cmd.spawn().context("spawning nix build")?;

    // With the UI active, a parser thread drains stderr into the shared
    // model while the render thread paints it; without a terminal, stderr
    // is simply inherited and nix formats its own progress.
    let display = if use_ui {
        let stderr = child.stderr.take().expect("stderr was piped");
        let model = Arc::new(Mutex::new(Model::new()));
        let parser = std::thread::spawn({
            let model = Arc::clone(&model);
            move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    model.lock().expect("model lock poisoned").apply_line(&line);
                }
            }
        });
        Some((Ui::spawn(Arc::clone(&model)), parser))
    } else {
        None
    };

    let mut stdout = String::new();
    child
        .stdout
        .take()
        .expect("stdout was piped")
        .read_to_string(&mut stdout)
        .context("reading nix build output")?;
    let status = child.wait().context("waiting for nix build")?;

    if let Some((ui, parser)) = display {
        let _ = parser.join();
        ui.finish(status.success());
    }
    if !status.success() {
        bail!("nix build failed ({status})");
    }

    let out_paths: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    match_paths(machines, &out_paths)
}

/// Match out paths to machines by the `nixos-system-<name>-` store name,
/// falling back to installable order when the name is ambiguous.
fn match_paths(machines: &[Machine], out_paths: &[&str]) -> Result<BTreeMap<String, String>> {
    let mut result = BTreeMap::new();
    for (index, machine) in machines.iter().enumerate() {
        let needle = format!("-nixos-system-{}-", machine.name);
        let named: Vec<&&str> = out_paths.iter().filter(|p| p.contains(&needle)).collect();
        let path = match named[..] {
            [only] => *only,
            _ => *out_paths.get(index).with_context(|| {
                format!(
                    "no out path for {}: nix printed {} paths for {} installables",
                    machine.name,
                    out_paths.len(),
                    machines.len()
                )
            })?,
        };
        result.insert(machine.name.clone(), path.to_string());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::target::Target;

    fn machine(name: &str) -> Machine {
        Machine {
            name: name.into(),
            target: Target::Local { sudo: true },
        }
    }

    #[test]
    fn matches_by_system_name() {
        let machines = [machine("tsugumi"), machine("saya")];
        // Deliberately reversed relative to the machine list.
        let paths = [
            "/nix/store/aaa-nixos-system-saya-26.05",
            "/nix/store/bbb-nixos-system-tsugumi-26.05",
        ];
        let map = match_paths(&machines, &paths).unwrap();
        assert_eq!(map["saya"], paths[0]);
        assert_eq!(map["tsugumi"], paths[1]);
    }

    #[test]
    fn falls_back_to_position() {
        let machines = [machine("a"), machine("b")];
        let paths = ["/nix/store/aaa-something", "/nix/store/bbb-other"];
        let map = match_paths(&machines, &paths).unwrap();
        assert_eq!(map["a"], paths[0]);
        assert_eq!(map["b"], paths[1]);
    }

    #[test]
    fn missing_path_is_an_error() {
        let machines = [machine("a"), machine("b")];
        assert!(match_paths(&machines, &["/nix/store/aaa-x"]).is_err());
    }
}
