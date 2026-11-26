//! Single-evaluation build of every selected machine's system toplevel.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, IsTerminal, Read};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use anyhow::{bail, Context, Result};

use crate::log_model::Model;
use crate::manifest::Machine;
use crate::ui::Ui;

fn installables_for(machines: &[Machine]) -> Vec<String> {
    machines
        .iter()
        .map(|m| {
            format!(
                ".#nixosConfigurations.{}.config.system.build.toplevel",
                m.name
            )
        })
        .collect()
}

/// Handle to a background `nix build` feeding a shared [`Model`].
///
/// Calling [`BuildHandle::cancel`] or dropping the handle kills the `nix` child
/// process and reaps its worker thread so no stray build lingers after quitting
/// the TUI.
pub struct BuildHandle {
    child: Arc<Mutex<Option<Child>>>,
    cancelled: Arc<AtomicBool>,
    result_rx: Receiver<Result<BTreeMap<String, String>>>,
    worker: Option<JoinHandle<()>>,
}

impl BuildHandle {
    /// Spawn a single-evaluation `nix build` with `--log-format internal-json`
    /// streaming stderr events into `model`.
    ///
    /// # Errors
    ///
    /// Errors if spawning `nix` fails.
    pub fn spawn(machines: &[Machine], model: Arc<Mutex<Model>>) -> Result<Self> {
        let installables = installables_for(machines);
        let mut cmd = Command::new("nix");
        cmd.args([
            "build",
            "--no-link",
            "--print-out-paths",
            "--log-format",
            "internal-json",
        ]);
        cmd.args(&installables);
        cmd.stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());

        let mut child = cmd.spawn().context("spawning nix build")?;
        let stdout = child.stdout.take().expect("stdout was piped");
        let stderr = child.stderr.take().expect("stderr was piped");

        let child_slot = Arc::new(Mutex::new(Some(child)));
        let cancelled = Arc::new(AtomicBool::new(false));
        let (tx, result_rx) = mpsc::channel();
        let machines_owned = machines.to_vec();

        // Parser drains stderr at full speed without holding `child_slot`.
        let parser = std::thread::spawn({
            let model = Arc::clone(&model);
            move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    if let Ok(mut guard) = model.lock() {
                        guard.apply_line(&line);
                    }
                }
            }
        });

        let worker = std::thread::spawn({
            let child_slot = Arc::clone(&child_slot);
            let cancelled = Arc::clone(&cancelled);
            move || {
                let outcome =
                    finish_build(stdout, parser, &child_slot, &cancelled, &machines_owned);
                let _ = tx.send(outcome);
            }
        });

        Ok(Self {
            child: child_slot,
            cancelled,
            result_rx,
            worker: Some(worker),
        })
    }

    /// Non-blocking check for build completion.
    pub fn try_take(&self) -> Option<Result<BTreeMap<String, String>>> {
        self.result_rx.try_recv().ok()
    }

    /// Kill the running `nix build` process if it is still alive.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
        if let Ok(mut guard) = self.child.lock() {
            if let Some(child) = guard.as_mut() {
                let _ = child.kill();
            }
        }
    }
}

impl Drop for BuildHandle {
    fn drop(&mut self) {
        self.cancel();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn finish_build(
    mut stdout: std::process::ChildStdout,
    parser: JoinHandle<()>,
    child_slot: &Mutex<Option<Child>>,
    cancelled: &AtomicBool,
    machines: &[Machine],
) -> Result<BTreeMap<String, String>> {
    let mut out = String::new();
    let read_res = stdout
        .read_to_string(&mut out)
        .context("reading nix build output");
    let _ = parser.join();

    let status = {
        let mut guard = child_slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match guard.take() {
            Some(mut child) => child.wait().context("waiting for nix build")?,
            None => bail!("nix build child handle was already taken"),
        }
    };

    if cancelled.load(Ordering::Relaxed) {
        bail!("nix build cancelled");
    }
    read_res?;
    if !status.success() {
        bail!("nix build failed ({status})");
    }
    let out_paths: Vec<&str> = out.lines().filter(|l| !l.is_empty()).collect();
    match_paths(machines, &out_paths)
}

/// Build all toplevels in one `nix build` invocation (one evaluation) and
/// return machine name -> out path.
///
/// # Errors
///
/// Errors if the build fails or an out path can't be matched to a machine.
pub fn build_toplevels(machines: &[Machine]) -> Result<BTreeMap<String, String>> {
    let installables = installables_for(machines);

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
