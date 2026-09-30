//! Refuse deploys that would roll back what a machine is running.
//!
//! Every system records the flake revision it was built from
//! (`system.configurationRevision`, set in `modules/nix-deploy.nix`). Before
//! building, each selected machine's running revision must be an ancestor of
//! the one being deployed, in this repository. Otherwise another checkout
//! deployed commits this one lacks, and deploying would silently undo them.
//! In a jj repository, a running commit that was since rewritten (squash,
//! describe, rebase) counts as contained if its change is an ancestor.

use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::manifest::Machine;

/// A flake revision: a commit, plus whether the tree had uncommitted changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rev {
    pub commit: String,
    pub dirty: bool,
}

impl Rev {
    /// Parse a `rev` or `dirtyRev` string (`<commit>` or `<commit>-dirty`).
    pub fn parse(s: &str) -> Option<Rev> {
        let (commit, dirty) = match s.strip_suffix("-dirty") {
            Some(commit) => (commit, true),
            None => (s, false),
        };
        let is_commit = commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit());
        is_commit.then(|| Rev {
            commit: commit.to_string(),
            dirty,
        })
    }

    fn short(&self) -> String {
        let short = &self.commit[..12];
        if self.dirty {
            format!("{short}-dirty")
        } else {
            short.to_string()
        }
    }
}

/// How a machine's running revision relates to the one being deployed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Running {
    /// The system records no revision (deployed before revisions were
    /// recorded, or by something other than this flake).
    Unrecorded,
    /// The running commit is an ancestor of (or equal to) the deployed one.
    Ancestor(Rev),
    /// The deployed commit contains a rewritten version of the running one
    /// (same jj change id, different commit).
    Rewritten(Rev),
    /// Both commits are known here, but the deployed one lacks the running one.
    Diverged(Rev),
    /// This repository doesn't have the running commit at all.
    Unknown(Rev),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Ok,
    Warn(String),
    Refuse(String),
}

/// Decide whether deploying over `running` is safe. `strict` (for
/// unattended deployers like agent-ship) also refuses when the running
/// revision is unrecorded or dirty, since then nobody can tell what would be
/// lost.
pub fn judge(machine: &str, running: &Running, strict: bool) -> Verdict {
    let lax = |msg: String| {
        if strict {
            Verdict::Refuse(msg)
        } else {
            Verdict::Warn(msg)
        }
    };
    match running {
        Running::Unrecorded => lax(format!(
            "{machine} doesn't record which revision it runs; \
             can't tell whether this deploy rolls anything back"
        )),
        Running::Rewritten(rev) => lax(format!(
            "{machine} runs {}, which has since been rewritten (jj); \
             this deploy replaces it with the rewritten version",
            rev.short()
        )),
        Running::Diverged(rev) => Verdict::Refuse(format!(
            "{machine} runs {}, which this checkout doesn't contain; deploying would \
             roll it back. Rebase onto it (e.g. `jj git fetch` and rebase) first",
            rev.short()
        )),
        Running::Unknown(rev) => Verdict::Refuse(format!(
            "{machine} runs {}, which this repository doesn't have: probably unpushed \
             work deployed from another checkout. Push it, then fetch and rebase onto it",
            rev.short()
        )),
        Running::Ancestor(rev) if rev.dirty => lax(format!(
            "{machine} runs uncommitted changes on top of {}; this deploy replaces them",
            &rev.commit[..12]
        )),
        Running::Ancestor(_) => Verdict::Ok,
    }
}

/// The revision a build in `root` would record, from `nix flake metadata`.
pub fn deploying_rev(root: &Path) -> Result<Rev> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Metadata {
        revision: Option<String>,
        dirty_revision: Option<String>,
    }
    let output = Command::new("nix")
        .args(["flake", "metadata", "--json", "--no-write-lock-file", "."])
        .current_dir(root)
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .context("spawning nix flake metadata")?;
    if !output.status.success() {
        bail!("nix flake metadata failed ({})", output.status);
    }
    let meta: Metadata =
        serde_json::from_slice(&output.stdout).context("parsing nix flake metadata")?;
    let raw = meta
        .revision
        .or(meta.dirty_revision)
        .context("the flake has no revision (is it a git checkout?)")?;
    Rev::parse(&raw).with_context(|| format!("unparseable flake revision {raw:?}"))
}

/// Classify `machine`'s running revision against `deploying`, using the git
/// history in `root`.
pub fn running(machine: &Machine, root: &Path, deploying: &Rev) -> Result<Running> {
    let json = machine
        .target
        .run_capture(&["nixos-version", "--json"], false)
        .context("reading the running revision")?;
    let Some(rev) = recorded_rev(&json)? else {
        return Ok(Running::Unrecorded);
    };
    let known = git(
        root,
        &["cat-file", "-e", &format!("{}^{{commit}}", rev.commit)],
    )?
    .success();
    if known {
        let ancestor = git(
            root,
            &[
                "merge-base",
                "--is-ancestor",
                &rev.commit,
                &deploying.commit,
            ],
        )?;
        match ancestor.code() {
            Some(0) => return Ok(Running::Ancestor(rev)),
            Some(1) => {}
            _ => bail!("git merge-base --is-ancestor failed ({ancestor})"),
        }
    }
    if root.join(".jj").is_dir() && jj_rewritten(root, &rev.commit, &deploying.commit)? {
        return Ok(Running::Rewritten(rev));
    }
    Ok(if known {
        Running::Diverged(rev)
    } else {
        Running::Unknown(rev)
    })
}

/// Whether `new`'s ancestors include a rewrite of `old`: a commit with
/// `old`'s jj change id. False if jj doesn't know `old`.
fn jj_rewritten(root: &Path, old: &str, new: &str) -> Result<bool> {
    let jj = |revset: &str, template: &str| {
        Command::new("jj")
            .args([
                "--ignore-working-copy",
                "--color=never",
                "log",
                "--no-graph",
            ])
            .args(["-r", revset, "-T", template])
            .current_dir(root)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .context("spawning jj log")
    };
    let change = jj(old, "change_id")?;
    if !change.status.success() {
        return Ok(false);
    }
    let change = String::from_utf8_lossy(&change.stdout).trim().to_string();
    let found = jj(
        &format!("ancestors({new}) & change_id({change})"),
        r#"commit_id ++ "\n""#,
    )?;
    if !found.status.success() {
        bail!("jj log failed looking for rewrites of {old}");
    }
    Ok(!found.stdout.is_empty())
}

/// The `configurationRevision` from `nixos-version --json`, if recorded.
fn recorded_rev(json: &str) -> Result<Option<Rev>> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Version {
        configuration_revision: Option<String>,
    }
    let version: Version = serde_json::from_str(json).context("parsing nixos-version --json")?;
    match version.configuration_revision {
        None => Ok(None),
        Some(raw) => Rev::parse(&raw)
            .map(Some)
            .with_context(|| format!("unparseable configurationRevision {raw:?}")),
    }
}

fn git(root: &Path, args: &[&str]) -> Result<std::process::ExitStatus> {
    Command::new("git")
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("spawning git {}", args.join(" ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "836503ae7a0da6da49cd7d16d2027299bffbb905";

    fn rev(dirty: bool) -> Rev {
        Rev {
            commit: A.into(),
            dirty,
        }
    }

    #[test]
    fn parses_clean_and_dirty_revs() {
        assert_eq!(Rev::parse(A), Some(rev(false)));
        assert_eq!(Rev::parse(&format!("{A}-dirty")), Some(rev(true)));
        assert_eq!(Rev::parse("836503ae"), None);
        assert_eq!(Rev::parse(""), None);
    }

    #[test]
    fn reads_configuration_revision() {
        let with = format!(r#"{{"configurationRevision":"{A}-dirty","nixosVersion":"26.11"}}"#);
        assert_eq!(recorded_rev(&with).unwrap(), Some(rev(true)));
        let without = r#"{"nixosVersion":"26.11","nixpkgsRevision":"ef34387"}"#;
        assert_eq!(recorded_rev(without).unwrap(), None);
    }

    #[test]
    fn ancestor_is_fine_in_both_modes() {
        for strict in [false, true] {
            assert_eq!(
                judge("m", &Running::Ancestor(rev(false)), strict),
                Verdict::Ok
            );
        }
    }

    #[test]
    fn rollbacks_are_always_refused() {
        for strict in [false, true] {
            for running in [Running::Diverged(rev(false)), Running::Unknown(rev(true))] {
                assert!(matches!(judge("m", &running, strict), Verdict::Refuse(_)));
            }
        }
    }

    #[test]
    fn dirty_unrecorded_or_rewritten_warns_unless_strict() {
        for running in [
            Running::Ancestor(rev(true)),
            Running::Unrecorded,
            Running::Rewritten(rev(false)),
        ] {
            assert!(matches!(judge("m", &running, false), Verdict::Warn(_)));
            assert!(matches!(judge("m", &running, true), Verdict::Refuse(_)));
        }
    }
}
