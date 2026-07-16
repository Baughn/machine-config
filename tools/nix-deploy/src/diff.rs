//! Closure comparison between the running system and a freshly built one,
//! producing a reboot-worthiness verdict.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use regex::Regex;
use serde::Deserialize;

use crate::target::Target;

/// Patterns that always warrant a reboot when their closure matches change.
/// Kernel, initrd, and the driver stack are caught by the structural links;
/// these cover reboot-worthy paths with no toplevel symlink.
const BUILTIN_PATTERNS: &[&str] = &["nvidia-x11", "linux-firmware"];

/// Symlinks inside a system toplevel whose retargeting means the running
/// state can no longer fully match the configuration without a reboot.
const STRUCTURAL_LINKS: &[&str] = &["kernel", "kernel-modules", "initrd", "systemd"];

/// File the nix-deploy module bakes into each toplevel with per-machine
/// extra patterns (`me.deploy.rebootPatterns`).
const PATTERNS_FILE: &str = "deploy-reboot-patterns.json";

/// Why (if at all) a reboot is recommended for one machine.
#[derive(Debug, Default)]
pub struct Verdict {
    pub reasons: Vec<String>,
}

impl Verdict {
    pub fn reboot_recommended(&self) -> bool {
        !self.reasons.is_empty()
    }
}

/// The store path of the system currently running on `target`.
pub fn current_system(target: &Target) -> Result<String> {
    target
        .run_capture(&["readlink", "-f", "/run/current-system"], false)
        .context("resolving /run/current-system")
}

/// Compare the running system `old` (resolved on `target`) against the
/// locally present `new` toplevel.
pub fn assess(target: &Target, old: &str, new: &str) -> Result<Verdict> {
    let mut reasons = structural_reasons(target, old, new);

    let patterns = load_patterns(new)?;
    let old_names = closure_names(target, old)
        .with_context(|| format!("listing the closure of {old} on {target}"))?;
    let new_names = closure_names(&Target::Local { sudo: false }, new)
        .with_context(|| format!("listing the closure of {new}"))?;
    reasons.extend(pattern_reasons(&patterns, &old_names, &new_names)?);

    Ok(Verdict { reasons })
}

fn structural_reasons(target: &Target, old: &str, new: &str) -> Vec<String> {
    let mut reasons = Vec::new();
    for link in STRUCTURAL_LINKS {
        let old_dest = resolve_link(target, &format!("{old}/{link}"));
        let new_dest = resolve_link(&Target::Local { sudo: false }, &format!("{new}/{link}"));
        match (old_dest, new_dest) {
            (Some(a), Some(b)) if a != b => {
                reasons.push(format!("{link}: {} -> {}", store_name(&a), store_name(&b)));
            }
            (Some(a), None) => reasons.push(format!("{link} removed (was {})", store_name(&a))),
            (None, Some(b)) => reasons.push(format!("{link} added ({})", store_name(&b))),
            _ => {}
        }
    }
    reasons
}

/// Resolve a symlink on the target, `None` if it doesn't exist there.
fn resolve_link(target: &Target, path: &str) -> Option<String> {
    match target.run_capture_unchecked(&["readlink", "-f", path], false) {
        Ok((true, dest)) if !dest.is_empty() => Some(dest),
        _ => None,
    }
}

/// Names (hash stripped) of every path in the closure of `path`, listed on
/// `target` so the old system's closure need not exist locally.
fn closure_names(target: &Target, path: &str) -> Result<BTreeSet<String>> {
    // nix-store rather than `nix path-info`: no experimental-features needed.
    let listing = target.run_capture(&["nix-store", "--query", "--requisites", path], false)?;
    Ok(listing.lines().map(|p| store_name(p).to_string()).collect())
}

/// Built-in patterns plus the per-machine extras baked into the toplevel.
fn load_patterns(new_toplevel: &str) -> Result<Vec<String>> {
    let mut patterns: Vec<String> = BUILTIN_PATTERNS.iter().map(|p| (*p).to_string()).collect();
    let path = Path::new(new_toplevel).join(PATTERNS_FILE);
    if path.exists() {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let extra = Vec::<String>::deserialize(&mut serde_json::Deserializer::from_str(&text))
            .with_context(|| format!("parsing {}", path.display()))?;
        patterns.extend(extra);
    }
    Ok(patterns)
}

/// Reasons from regex patterns whose matched store names differ between the
/// old and new closures.
fn pattern_reasons(
    patterns: &[String],
    old_names: &BTreeSet<String>,
    new_names: &BTreeSet<String>,
) -> Result<Vec<String>> {
    let mut reasons = Vec::new();
    for pattern in patterns {
        let re = Regex::new(pattern).with_context(|| format!("invalid pattern {pattern:?}"))?;
        let old_matched: BTreeSet<&String> = old_names.iter().filter(|n| re.is_match(n)).collect();
        let new_matched: BTreeSet<&String> = new_names.iter().filter(|n| re.is_match(n)).collect();
        if old_matched != new_matched {
            let removed: Vec<&str> = old_matched
                .difference(&new_matched)
                .map(|s| s.as_str())
                .collect();
            let added: Vec<&str> = new_matched
                .difference(&old_matched)
                .map(|s| s.as_str())
                .collect();
            reasons.push(format!(
                "pattern {pattern:?}: {} -> {}",
                summarize(&removed),
                summarize(&added)
            ));
        }
    }
    Ok(reasons)
}

fn summarize(names: &[&str]) -> String {
    const SHOWN: usize = 3;
    match names.len() {
        0 => "(none)".to_string(),
        n if n <= SHOWN => names.join(", "),
        n => format!("{}, ... ({n} total)", names[..SHOWN].join(", ")),
    }
}

/// The name part of a store path: `/nix/store/<hash>-foo-1.2` → `foo-1.2`.
pub fn store_name(path: &str) -> &str {
    let base = path.rsplit('/').next().unwrap_or(path);
    base.split_once('-').map_or(base, |(_, name)| name)
}

/// Print a human-readable package diff via `nvd`, when it is installed and
/// the old closure is present locally. Purely informational; never fails.
pub fn show_nvd_diff(old: &str, new: &str) {
    if !Path::new(old).exists() {
        eprintln!("(old system not in the local store; skipping nvd package diff)");
        return;
    }
    match Command::new("nvd").args(["diff", old, new]).status() {
        Ok(status) if status.success() => {}
        Ok(status) => eprintln!("(nvd diff failed: {status})"),
        Err(_) => eprintln!("(nvd not installed; skipping package diff)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_store_prefix() {
        assert_eq!(store_name("/nix/store/abc123-kwin-6.6.3"), "kwin-6.6.3");
        assert_eq!(store_name("abc123-linux-6.15.4"), "linux-6.15.4");
        assert_eq!(store_name("no-slash"), "slash");
    }

    fn names(list: &[&str]) -> BTreeSet<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn pattern_flags_version_changes() {
        let old = names(&["kwin-6.6.3", "bash-5.2", "nvidia-x11-570.1"]);
        let new = names(&["kwin-6.6.4", "bash-5.3", "nvidia-x11-570.1"]);
        let patterns = vec!["kwin".to_string(), "nvidia-x11".to_string()];
        let reasons = pattern_reasons(&patterns, &old, &new).unwrap();
        assert_eq!(reasons.len(), 1);
        assert!(reasons[0].contains("kwin-6.6.3"));
        assert!(reasons[0].contains("kwin-6.6.4"));
    }

    #[test]
    fn pattern_ignores_unchanged_matches() {
        let old = names(&["nvidia-x11-570.1", "firefox-100"]);
        let new = names(&["nvidia-x11-570.1", "firefox-101"]);
        let patterns = vec!["nvidia-x11".to_string()];
        assert!(pattern_reasons(&patterns, &old, &new).unwrap().is_empty());
    }

    #[test]
    fn invalid_pattern_errors() {
        let empty = BTreeSet::new();
        assert!(pattern_reasons(&["(".to_string()], &empty, &empty).is_err());
    }

    #[test]
    fn summarize_caps_long_lists() {
        assert_eq!(summarize(&[]), "(none)");
        assert_eq!(summarize(&["a", "b"]), "a, b");
        assert_eq!(
            summarize(&["a", "b", "c", "d", "e"]),
            "a, b, c, ... (5 total)"
        );
    }
}
