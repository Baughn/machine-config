//! Command execution on a deploy target, local or over ssh.

use std::fmt;
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

/// Where and how to run commands for one machine.
///
/// `sudo` controls whether root-level commands are prefixed with `sudo`;
/// it is false when the ssh destination is already root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Local { sudo: bool },
    Ssh { dest: String, sudo: bool },
}

impl Target {
    pub fn is_local(&self) -> bool {
        matches!(self, Target::Local { .. })
    }

    /// The ssh destination string, if this is a remote target.
    pub fn ssh_dest(&self) -> Option<&str> {
        match self {
            Target::Local { .. } => None,
            Target::Ssh { dest, .. } => Some(dest),
        }
    }

    /// Build a `Command` running `argv` on this target.
    ///
    /// With `as_root`, the command is wrapped in `sudo` unless the target
    /// already runs as root. Arguments must not contain shell metacharacters
    /// that matter after ssh's argv-join (store paths and fixed flags are safe).
    pub fn command(&self, argv: &[&str], as_root: bool) -> Command {
        assert!(!argv.is_empty(), "empty argv");
        match self {
            Target::Local { sudo } => {
                if as_root && *sudo {
                    let mut cmd = Command::new("sudo");
                    cmd.args(argv);
                    cmd
                } else {
                    let mut cmd = Command::new(argv[0]);
                    cmd.args(&argv[1..]);
                    cmd
                }
            }
            Target::Ssh { dest, sudo } => {
                let mut cmd = Command::new("ssh");
                cmd.args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=10", dest, "--"]);
                if as_root && *sudo {
                    cmd.arg("sudo");
                }
                cmd.args(argv);
                cmd
            }
        }
    }

    /// Run `argv` and return trimmed stdout; non-zero exit is an error.
    pub fn run_capture(&self, argv: &[&str], as_root: bool) -> Result<String> {
        let output = self
            .command(argv, as_root)
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("spawning `{}` on {self}", argv.join(" ")))?;
        if !output.status.success() {
            bail!(
                "`{}` on {self} failed ({}): {}",
                argv.join(" "),
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Run `argv` and return (success, trimmed stdout) without treating
    /// non-zero exit as an error. Spawn failures still error.
    pub fn run_capture_unchecked(&self, argv: &[&str], as_root: bool) -> Result<(bool, String)> {
        let output = self
            .command(argv, as_root)
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("spawning `{}` on {self}", argv.join(" ")))?;
        Ok((
            output.status.success(),
            String::from_utf8_lossy(&output.stdout).trim().to_string(),
        ))
    }

    /// Run `argv` with stdout/stderr passed through to the terminal.
    pub fn run_streamed(&self, argv: &[&str], as_root: bool) -> Result<()> {
        let status = self
            .command(argv, as_root)
            .stdin(Stdio::null())
            .status()
            .with_context(|| format!("spawning `{}` on {self}", argv.join(" ")))?;
        if !status.success() {
            bail!("`{}` on {self} failed ({status})", argv.join(" "));
        }
        Ok(())
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Target::Local { .. } => write!(f, "local"),
            Target::Ssh { dest, .. } => write!(f, "{dest}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv_of(cmd: &Command) -> Vec<String> {
        std::iter::once(cmd.get_program().to_string_lossy().into_owned())
            .chain(cmd.get_args().map(|a| a.to_string_lossy().into_owned()))
            .collect()
    }

    #[test]
    fn local_root_uses_sudo() {
        let t = Target::Local { sudo: true };
        assert_eq!(
            argv_of(&t.command(&["ls", "/x"], true)),
            ["sudo", "ls", "/x"]
        );
        assert_eq!(argv_of(&t.command(&["ls", "/x"], false)), ["ls", "/x"]);
    }

    #[test]
    fn ssh_root_target_skips_sudo() {
        let t = Target::Ssh {
            dest: "root@host".into(),
            sudo: false,
        };
        let argv = argv_of(&t.command(&["reboot"], true));
        assert!(!argv.contains(&"sudo".to_string()));
        assert_eq!(argv[0], "ssh");
    }

    #[test]
    fn ssh_user_target_uses_sudo_for_root() {
        let t = Target::Ssh {
            dest: "svein@host".into(),
            sudo: true,
        };
        let argv = argv_of(&t.command(&["reboot"], true));
        let pos = argv.iter().position(|a| a == "--").expect("ssh separator");
        assert_eq!(&argv[pos + 1..], ["sudo", "reboot"]);
    }
}
