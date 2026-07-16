//! The `deploy.toml` machine manifest.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::target::Target;

pub const MANIFEST_NAME: &str = "deploy.toml";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestFile {
    #[serde(default)]
    settings: Settings,
    machines: BTreeMap<String, MachineEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MachineEntry {
    /// ssh destination (e.g. `svein@tsugumi.local`); required for remote machines.
    target: Option<String>,
    /// This machine is the one the tool runs on; deploys via sudo, no ssh.
    #[serde(default)]
    local: bool,
    /// Deploy order; the local machine must sort last.
    order: u32,
    /// Wrap root commands in sudo. Defaults to true, except for ssh
    /// destinations already logging in as root.
    sudo: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// How long to wait for a remote machine to come back after a reboot.
    #[serde(default = "default_reboot_timeout")]
    pub reboot_timeout_secs: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            reboot_timeout_secs: default_reboot_timeout(),
        }
    }
}

fn default_reboot_timeout() -> u64 {
    300
}

#[derive(Debug, Clone)]
pub struct Machine {
    pub name: String,
    pub target: Target,
}

#[derive(Debug)]
pub struct Manifest {
    pub settings: Settings,
    /// All machines, in deploy order (remotes first, local last).
    pub machines: Vec<Machine>,
}

/// Walk up from `start` to the directory containing `deploy.toml`.
///
/// # Errors
///
/// Errors if no ancestor of `start` contains a manifest.
pub fn find_repo_root(start: &Path) -> Result<PathBuf> {
    for dir in start.ancestors() {
        if dir.join(MANIFEST_NAME).is_file() {
            return Ok(dir.to_path_buf());
        }
    }
    bail!(
        "no {MANIFEST_NAME} found in {} or any parent; run from the nixos repo",
        start.display()
    );
}

/// Load and validate the manifest in `root`.
///
/// # Errors
///
/// Errors on parse failures, remote machines without a target, local
/// machines with one, or a local machine ordered before a remote one.
pub fn load(root: &Path) -> Result<Manifest> {
    let path = root.join(MANIFEST_NAME);
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    parse(&text).with_context(|| format!("parsing {}", path.display()))
}

fn parse(text: &str) -> Result<Manifest> {
    let file: ManifestFile = toml::from_str(text)?;
    let mut ordered: Vec<(u32, Machine)> = Vec::with_capacity(file.machines.len());
    for (name, entry) in file.machines {
        let target = match (entry.local, entry.target) {
            (true, Some(_)) => bail!("machine {name}: `local` and `target` are exclusive"),
            (true, None) => Target::Local {
                sudo: entry.sudo.unwrap_or(true),
            },
            (false, None) => bail!("machine {name}: remote machine needs a `target`"),
            (false, Some(dest)) => {
                let sudo = entry.sudo.unwrap_or_else(|| !dest.starts_with("root@"));
                Target::Ssh { dest, sudo }
            }
        };
        ordered.push((entry.order, Machine { name, target }));
    }
    ordered.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.name.cmp(&b.1.name)));

    let machines: Vec<Machine> = ordered.into_iter().map(|(_, m)| m).collect();
    if let Some(first_local) = machines.iter().position(|m| m.target.is_local()) {
        if machines[first_local..].iter().any(|m| !m.target.is_local()) {
            bail!("the local machine must have the highest `order` (it deploys last)");
        }
    }
    Ok(Manifest {
        settings: file.settings,
        machines,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
        [machines.tsugumi]
        target = "svein@tsugumi.local"
        order = 1

        [machines.saya]
        local = true
        order = 2
    "#;

    #[test]
    fn parses_and_orders() {
        let m = parse(GOOD).unwrap();
        let names: Vec<&str> = m.machines.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["tsugumi", "saya"]);
        assert_eq!(
            m.machines[0].target,
            Target::Ssh {
                dest: "svein@tsugumi.local".into(),
                sudo: true
            }
        );
        assert_eq!(m.machines[1].target, Target::Local { sudo: true });
        assert_eq!(m.settings.reboot_timeout_secs, 300);
    }

    #[test]
    fn root_ssh_disables_sudo() {
        let m = parse(
            r#"
            [machines.a]
            target = "root@a"
            order = 1
            "#,
        )
        .unwrap();
        assert_eq!(
            m.machines[0].target,
            Target::Ssh {
                dest: "root@a".into(),
                sudo: false
            }
        );
    }

    #[test]
    fn local_must_be_last() {
        let err = parse(
            r#"
            [machines.a]
            local = true
            order = 1

            [machines.b]
            target = "svein@b"
            order = 2
            "#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("highest `order`"));
    }

    #[test]
    fn remote_needs_target() {
        assert!(parse("[machines.a]\norder = 1\n").is_err());
    }
}
