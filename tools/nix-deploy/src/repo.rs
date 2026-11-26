//! Jujutsu and Git repository inspection, rebase, commit hygiene, rollback, and push.
//!
//! Supports both colocated Jujutsu (`jj`) checkouts (the normal desktop/server workflow)
//! and plain Git checkouts (such as unattended `agent-ship` clones).

use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

/// Tab-delimited `jj log` template used to parse `CommitSummary` records.
///
/// The commit subject is placed last so `splitn(4, '\t')` preserves any tab characters
/// that might appear inside a commit description.
const JJ_COMMIT_TEMPLATE: &str = concat!(
    r#"commit_id ++ "\t" ++ commit_id.short() ++ "\t" ++ "#,
    r#"change_id.short() ++ "\t" ++ description.first_line() ++ "\n""#,
);

/// Summary of a single repository commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitSummary {
    /// Full 40-character hexadecimal Git commit ID.
    pub commit_id: String,
    /// Abbreviated commit ID for display.
    pub short_id: String,
    /// Jujutsu change ID (empty for plain Git queries such as [`commits_between`]).
    pub change_id: String,
    /// Trimmed first line of the commit description.
    pub description: String,
}

/// Snapshot of repository state relevant to deploying from a checkout.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoState {
    /// Whether `<root>/.jj` exists. When `false`, all other fields are empty/default.
    pub is_jj: bool,
    /// Current Jujutsu operation ID, usable with [`restore_op`] to roll back changes.
    pub op_id: Option<String>,
    /// Commits on `trunk()` not yet in ancestors of `@` (`@..trunk()`).
    pub incoming_commits: Vec<CommitSummary>,
    /// Whether the working-copy commit `@` has tree changes (`!empty()`).
    pub wc_dirty: bool,
    /// Repository-relative paths changed in `@`.
    pub wc_changed_files: Vec<String>,
    /// Trimmed description of `@`.
    pub wc_description: String,
    /// Whether `@-` is a single mutable parent whose first description line is `"Bumps"`.
    pub parent_is_mutable_bumps: bool,
    /// Non-empty local commits ahead of `trunk()` (`(trunk()..@) & ~empty()`).
    pub unpushed_commits: Vec<CommitSummary>,
}

impl RepoState {
    /// Returns `true` if `@` is dirty and `flake.lock` is the sole modified path.
    pub fn wc_only_flake_lock(&self) -> bool {
        self.wc_dirty
            && self.wc_changed_files.len() == 1
            && self.wc_changed_files[0] == "flake.lock"
    }

    /// Returns `true` if `@` already has a non-empty commit description.
    pub fn wc_is_named(&self) -> bool {
        !self.wc_description.trim().is_empty()
    }
}

/// Outcome of recording a `flake.lock`-only change via [`commit_flake_lock_bumps`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BumpCommitOutcome {
    /// Squashed `@` into the existing mutable `"Bumps"` parent commit (`@-`).
    SquashedIntoExisting,
    /// Described `@` as `"Bumps"` and created a fresh empty `@` on top.
    CreatedNew,
}

/// Inspect repository state at `root`.
///
/// If `<root>/.jj` does not exist (for example, a plain Git clone used by `agent-ship`),
/// returns a default [`RepoState`] with `is_jj == false` without invoking `jj`.
///
/// In a Jujutsu checkout, the first query snapshots the working copy so dirty files on
/// disk are reflected accurately, and all subsequent queries read that same snapshot via
/// `--ignore-working-copy`.
///
/// # Arguments
///
/// * `root` - Path to the repository root.
///
/// # Returns
///
/// A populated [`RepoState`].
///
/// # Errors
///
/// Returns an error if `jj` cannot be spawned or any inspection query fails.
pub fn inspect(root: &Path) -> Result<RepoState> {
    if !is_jj_repo(root) {
        return Ok(RepoState::default());
    }

    // Snapshot the working copy once on the first query so subsequent read-only queries
    // observe the exact same operation and tree state.
    let wc_raw = run_jj(
        root,
        false,
        &[
            "log",
            "-r",
            "@",
            "--no-graph",
            "-T",
            r#"if(empty, "clean", "dirty") ++ "\n" ++ description"#,
        ],
    )
    .context("inspecting working copy status")?;
    let (wc_dirty, wc_description) = parse_wc_status(&wc_raw)?;

    let op_id = Some(current_op_id(root)?);
    let wc_changed_files = if wc_dirty {
        let diff_raw = run_jj(root, true, &["diff", "-r", "@", "--summary"])
            .context("listing working copy changed files")?;
        parse_diff_summary(&diff_raw)
    } else {
        Vec::new()
    };

    let incoming_commits =
        query_commits(root, true, "@..trunk()").context("querying incoming commits")?;
    let parent_is_mutable_bumps =
        is_parent_mutable_bumps(root, true).context("checking parent commit")?;
    let unpushed_commits = query_commits(root, true, "(trunk()..@) & ~empty()")
        .context("querying unpushed commits")?;

    Ok(RepoState {
        is_jj: true,
        op_id,
        incoming_commits,
        wc_dirty,
        wc_changed_files,
        wc_description,
        parent_is_mutable_bumps,
        unpushed_commits,
    })
}

/// Rebase local work (or an empty `@` sitting behind `trunk()`) onto `trunk()`.
///
/// After rebasing, checks `conflicts() & (trunk()..@ | @)` because Jujutsu records
/// conflicts as first-class commit state and exits 0 even when conflicts occur.
///
/// # Arguments
///
/// * `root` - Path to the Jujutsu repository root.
///
/// # Errors
///
/// Returns an error if `root` is not a Jujutsu checkout, if `jj rebase` fails, or if
/// rebasing introduces conflicts that require manual resolution.
pub fn rebase_onto_trunk(root: &Path) -> Result<()> {
    ensure_jj_repo(root)?;

    let local_non_wc = run_jj(
        root,
        false,
        &[
            "log",
            "-r",
            "(remote_bookmarks()..@) & ~@",
            "--no-graph",
            "-T",
            r#"commit_id ++ "\n""#,
        ],
    )
    .context("checking for local commits to rebase")?;

    if !local_non_wc.trim().is_empty() {
        run_jj(
            root,
            false,
            &["rebase", "-r", "remote_bookmarks()..@", "-d", "trunk()"],
        )
        .context("rebasing local commits onto trunk()")?;
    } else {
        run_jj(root, false, &["rebase", "-r", "@", "-d", "trunk()"])
            .context("rebasing working copy onto trunk()")?;
    }

    let conflict_raw = run_jj(
        root,
        true,
        &[
            "log",
            "-r",
            "conflicts() & (trunk()..@ | @)",
            "--no-graph",
            "-T",
            r#"commit_id.short() ++ "\t" ++ description.first_line() ++ "\n""#,
        ],
    )
    .context("checking for rebase conflicts")?;

    let conflicts = parse_conflict_list(&conflict_raw);
    if !conflicts.is_empty() {
        bail!(
            "rebase onto trunk produced conflicts in {}; \
             resolve manually (e.g. `jj resolve`) before deploying",
            conflicts.join(", ")
        );
    }

    Ok(())
}

/// Set the description of `@` to `message` and start a fresh empty working-copy commit.
///
/// Used when `@` is dirty and unnamed so subsequent `nix build` invocations record a
/// clean `self.rev` instead of `-dirty`.
///
/// # Arguments
///
/// * `root` - Path to the Jujutsu repository root.
/// * `message` - Non-empty commit message for `@`.
///
/// # Errors
///
/// Returns an error if `root` is not a Jujutsu checkout, `message` is blank, or `jj` fails.
pub fn describe_and_new(root: &Path, message: &str) -> Result<()> {
    ensure_jj_repo(root)?;
    let trimmed = message.trim();
    if trimmed.is_empty() {
        bail!("commit description must not be empty");
    }
    run_jj(root, false, &["describe", "-m", trimmed]).context("describing working copy")?;
    run_jj(root, false, &["new"]).context("creating new empty working copy commit")?;
    Ok(())
}

/// Start a fresh empty working-copy commit on top of an already-named `@`.
///
/// # Arguments
///
/// * `root` - Path to the Jujutsu repository root.
///
/// # Errors
///
/// Returns an error if `root` is not a Jujutsu checkout or `jj new` fails.
pub fn new_commit(root: &Path) -> Result<()> {
    ensure_jj_repo(root)?;
    run_jj(root, false, &["new"]).context("creating new empty working copy commit")?;
    Ok(())
}

/// Record a `flake.lock`-only working-copy change as a `"Bumps"` commit.
///
/// Re-checks whether `@-` is a single mutable `"Bumps"` commit. If so, squashes `@` into
/// `@-` keeping its message; otherwise describes `@` as `"Bumps"` and creates a new empty
/// `@` on top. In both cases `@` is left empty so `nix build` sees a clean revision.
///
/// # Arguments
///
/// * `root` - Path to the Jujutsu repository root.
///
/// # Returns
///
/// Whether the change was squashed into an existing mutable `"Bumps"` commit or recorded
/// as a new `"Bumps"` commit.
///
/// # Errors
///
/// Returns an error if `root` is not a Jujutsu checkout or any `jj` command fails.
pub fn commit_flake_lock_bumps(root: &Path) -> Result<BumpCommitOutcome> {
    ensure_jj_repo(root)?;

    if is_parent_mutable_bumps(root, false).context("checking whether @- is mutable Bumps")? {
        run_jj(
            root,
            false,
            &["squash", "--into", "@-", "--use-destination-message"],
        )
        .context("squashing flake.lock into mutable Bumps commit")?;
        Ok(BumpCommitOutcome::SquashedIntoExisting)
    } else {
        run_jj(root, false, &["describe", "-m", "Bumps"])
            .context("describing flake.lock update as Bumps")?;
        run_jj(root, false, &["new"]).context("creating new empty working copy commit")?;
        Ok(BumpCommitOutcome::CreatedNew)
    }
}

/// Read the current Jujutsu operation ID.
///
/// # Arguments
///
/// * `root` - Path to the Jujutsu repository root.
///
/// # Returns
///
/// The full hexadecimal operation ID of the latest operation.
///
/// # Errors
///
/// Returns an error if `root` is not a Jujutsu checkout, `jj op log` fails, or the output
/// is empty.
pub fn current_op_id(root: &Path) -> Result<String> {
    ensure_jj_repo(root)?;
    let raw = run_jj(
        root,
        true,
        &["op", "log", "--limit", "1", "--no-graph", "-T", "id"],
    )
    .context("reading current jj operation id")?;
    let id = raw.trim().to_string();
    if id.is_empty() {
        bail!("jj op log returned an empty operation id");
    }
    Ok(id)
}

/// Restore the repository (including the working copy) to a previously captured operation.
///
/// # Arguments
///
/// * `root` - Path to the Jujutsu repository root.
/// * `op_id` - Operation ID returned by [`current_op_id`] or [`RepoState::op_id`].
///
/// # Errors
///
/// Returns an error if `root` is not a Jujutsu checkout, `op_id` is empty, or `jj op restore`
/// fails.
pub fn restore_op(root: &Path, op_id: &str) -> Result<()> {
    ensure_jj_repo(root)?;
    let trimmed = op_id.trim();
    if trimmed.is_empty() {
        bail!("operation id must not be empty");
    }
    run_jj(root, false, &["op", "restore", trimmed])
        .with_context(|| format!("restoring jj operation {trimmed}"))?;
    Ok(())
}

/// Advance the `master` bookmark to the latest non-empty named ancestor of `@-` and push.
///
/// Mirrors `push.sh` without squashing already-deployed commits or sending notifications.
///
/// # Arguments
///
/// * `root` - Path to the Jujutsu repository root.
///
/// # Returns
///
/// A concise human-readable summary of the push result.
///
/// # Errors
///
/// Returns an error if `root` is not a Jujutsu checkout or either `jj bookmark set` or
/// `jj git push` fails.
pub fn push_master(root: &Path) -> Result<String> {
    ensure_jj_repo(root)?;
    run_jj(
        root,
        false,
        &[
            "bookmark",
            "set",
            "master",
            "-r",
            r#"latest(ancestors(@-) & ~empty() & ~description(exact:""))"#,
        ],
    )
    .context("setting master bookmark")?;

    let (stdout, stderr) = run_jj_output(root, false, &["git", "push"])
        .context("pushing master bookmark to git remote")?;
    Ok(format_push_summary(&stdout, &stderr))
}

/// List commits reachable from `new_rev` that are not reachable from `old_rev`.
///
/// Uses `git log` so it works identically in colocated Jujutsu and plain Git repositories.
///
/// # Arguments
///
/// * `root` - Path to the repository root.
/// * `old_rev` - Base commit revision (excluded).
/// * `new_rev` - Target commit revision (included).
///
/// # Returns
///
/// Commits in `old_rev..new_rev` ordered newest-first, with empty `change_id` fields.
///
/// # Errors
///
/// Returns an error if either revision is empty/invalid or `git log` fails.
pub fn commits_between(root: &Path, old_rev: &str, new_rev: &str) -> Result<Vec<CommitSummary>> {
    let old = old_rev.trim();
    let new = new_rev.trim();
    if old.is_empty() || new.is_empty() {
        bail!("old_rev and new_rev must not be empty");
    }
    if old.starts_with('-') || new.starts_with('-') {
        bail!("invalid revision specification: {old:?}..{new:?}");
    }
    let range = format!("{old}..{new}");
    let out = run_git(root, &["log", "--format=%H%x1f%h%x1f%s", &range])
        .with_context(|| format!("listing commits in {range}"))?;
    Ok(parse_git_commits(&out))
}

fn is_jj_repo(root: &Path) -> bool {
    root.join(".jj").is_dir()
}

fn ensure_jj_repo(root: &Path) -> Result<()> {
    if !is_jj_repo(root) {
        bail!("not a Jujutsu repository: {}", root.display());
    }
    Ok(())
}

fn query_commits(root: &Path, ignore_wc: bool, revset: &str) -> Result<Vec<CommitSummary>> {
    let raw = run_jj(
        root,
        ignore_wc,
        &["log", "-r", revset, "--no-graph", "-T", JJ_COMMIT_TEMPLATE],
    )?;
    Ok(parse_jj_commits(&raw))
}

fn is_parent_mutable_bumps(root: &Path, ignore_wc: bool) -> Result<bool> {
    let raw = run_jj(
        root,
        ignore_wc,
        &[
            "log",
            "-r",
            "@-",
            "--no-graph",
            "-T",
            r#"if(immutable, "0", "1") ++ "\t" ++ description.first_line() ++ "\n""#,
        ],
    )?;
    Ok(parse_parent_mutable_bumps(&raw))
}

fn run_jj(root: &Path, ignore_wc: bool, args: &[&str]) -> Result<String> {
    let (stdout, _stderr) = run_jj_output(root, ignore_wc, args)?;
    Ok(stdout)
}

fn run_jj_output(root: &Path, ignore_wc: bool, args: &[&str]) -> Result<(String, String)> {
    let mut cmd = Command::new("jj");
    cmd.args(["--no-pager", "--color=never"]);
    if ignore_wc {
        cmd.arg("--ignore-working-copy");
    }
    let output = cmd
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("spawning jj {}", args.join(" ")))?;

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if !output.status.success() {
        let detail = if !stderr.trim().is_empty() {
            stderr.trim()
        } else {
            stdout.trim()
        };
        bail!(
            "jj {} failed ({}): {}",
            args.join(" "),
            output.status,
            detail
        );
    }
    Ok((stdout, stderr))
}

fn run_git(root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("spawning git {}", args.join(" ")))?;

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if !output.status.success() {
        let detail = if !stderr.trim().is_empty() {
            stderr.trim()
        } else {
            stdout.trim()
        };
        bail!(
            "git {} failed ({}): {}",
            args.join(" "),
            output.status,
            detail
        );
    }
    Ok(stdout)
}

fn parse_wc_status(raw: &str) -> Result<(bool, String)> {
    let (status, desc) = match raw.split_once('\n') {
        Some((first, rest)) => (first.trim(), rest.trim().to_string()),
        None => (raw.trim(), String::new()),
    };
    match status {
        "clean" => Ok((false, desc)),
        "dirty" => Ok((true, desc)),
        other => bail!("unexpected working-copy status marker from jj: {other:?}"),
    }
}

fn parse_jj_commits(raw: &str) -> Vec<CommitSummary> {
    let mut commits = Vec::new();
    for line in raw.lines() {
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(4, '\t');
        let (Some(commit_id), Some(short_id), Some(change_id), Some(desc)) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        commits.push(CommitSummary {
            commit_id: commit_id.to_string(),
            short_id: short_id.to_string(),
            change_id: change_id.to_string(),
            description: desc.trim().to_string(),
        });
    }
    commits
}

fn parse_git_commits(raw: &str) -> Vec<CommitSummary> {
    let mut commits = Vec::new();
    for line in raw.lines() {
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(3, '\x1f');
        let (Some(commit_id), Some(short_id), Some(desc)) =
            (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        commits.push(CommitSummary {
            commit_id: commit_id.to_string(),
            short_id: short_id.to_string(),
            change_id: String::new(),
            description: desc.trim().to_string(),
        });
    }
    commits
}

fn parse_diff_summary(raw: &str) -> Vec<String> {
    raw.lines()
        .filter_map(|line| {
            let trimmed = line.trim_end_matches('\r');
            if trimmed.is_empty() {
                return None;
            }
            let bytes = trimmed.as_bytes();
            let path = if bytes.len() >= 2 && bytes[0].is_ascii_uppercase() && bytes[1] == b' ' {
                &trimmed[2..]
            } else {
                trimmed
            };
            let path = path.trim();
            (!path.is_empty()).then(|| path.to_string())
        })
        .collect()
}

fn parse_parent_mutable_bumps(raw: &str) -> bool {
    let mut lines = raw.lines().filter(|line| !line.is_empty());
    let Some(first) = lines.next() else {
        return false;
    };
    // Merge commits have multiple parents; never treat a multi-parent @ as squashing into Bumps.
    if lines.next().is_some() {
        return false;
    }
    let Some((mutable_flag, desc)) = first.split_once('\t') else {
        return false;
    };
    mutable_flag == "1" && desc.trim() == "Bumps"
}

fn parse_conflict_list(raw: &str) -> Vec<String> {
    raw.lines()
        .filter(|line| !line.is_empty())
        .map(|line| match line.split_once('\t') {
            Some((id, desc)) => {
                let desc = desc.trim();
                if desc.is_empty() {
                    format!("{id} (unnamed)")
                } else {
                    format!("{id} ({desc})")
                }
            }
            None => line.trim().to_string(),
        })
        .collect()
}

fn format_push_summary(stdout: &str, stderr: &str) -> String {
    let lines: Vec<&str> = stderr
        .lines()
        .chain(stdout.lines())
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if lines.is_empty() {
        "pushed master".to_string()
    } else {
        lines.join("; ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    // ---- Pure unit tests ----

    #[test]
    fn wc_only_flake_lock_checks_dirty_and_exact_path() {
        let mut state = RepoState {
            is_jj: true,
            wc_dirty: true,
            wc_changed_files: vec!["flake.lock".into()],
            ..Default::default()
        };
        assert!(state.wc_only_flake_lock());

        state.wc_dirty = false;
        assert!(!state.wc_only_flake_lock());

        state.wc_dirty = true;
        state.wc_changed_files = vec!["flake.lock".into(), "flake.nix".into()];
        assert!(!state.wc_only_flake_lock());

        state.wc_changed_files = vec!["sub/flake.lock".into()];
        assert!(!state.wc_only_flake_lock());

        state.wc_changed_files = vec!["{other => flake.lock}".into()];
        assert!(!state.wc_only_flake_lock());

        state.wc_changed_files.clear();
        assert!(!state.wc_only_flake_lock());
    }

    #[test]
    fn wc_is_named_detects_blank_and_named_descriptions() {
        let mut state = RepoState::default();
        assert!(!state.wc_is_named());

        state.wc_description = "   \n\t  ".into();
        assert!(!state.wc_is_named());

        state.wc_description = "Add monitoring rule".into();
        assert!(state.wc_is_named());
    }

    #[test]
    fn parses_wc_status_clean_dirty_and_multiline_description() {
        assert_eq!(parse_wc_status("clean\n").unwrap(), (false, String::new()));
        assert_eq!(parse_wc_status("clean").unwrap(), (false, String::new()));
        assert_eq!(
            parse_wc_status("dirty\nTitle line\n\nBody text\n").unwrap(),
            (true, "Title line\n\nBody text".to_string())
        );
        assert!(parse_wc_status("bogus\nfoo").is_err());
    }

    #[test]
    fn parses_jj_commits_including_empty_and_tabbed_descriptions() {
        let raw = concat!(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\taaaaaaaa\tkmkuslsw\tFix\tspacing\n",
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\tbbbbbbbb\tnopqrstu\t\n",
        );
        let parsed = parse_jj_commits(raw);
        assert_eq!(parsed.len(), 2);
        assert_eq!(
            parsed[0],
            CommitSummary {
                commit_id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
                short_id: "aaaaaaaa".into(),
                change_id: "kmkuslsw".into(),
                description: "Fix\tspacing".into(),
            }
        );
        assert_eq!(
            parsed[1],
            CommitSummary {
                commit_id: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
                short_id: "bbbbbbbb".into(),
                change_id: "nopqrstu".into(),
                description: String::new(),
            }
        );
    }

    #[test]
    fn parses_git_commits_with_unit_separator() {
        let raw = "1111111111111111111111111111111111111111\x1f1111111\x1fInitial commit\n\
                   2222222222222222222222222222222222222222\x1f2222222\x1fBumps\n";
        let parsed = parse_git_commits(raw);
        assert_eq!(
            parsed,
            vec![
                CommitSummary {
                    commit_id: "1111111111111111111111111111111111111111".into(),
                    short_id: "1111111".into(),
                    change_id: String::new(),
                    description: "Initial commit".into(),
                },
                CommitSummary {
                    commit_id: "2222222222222222222222222222222222222222".into(),
                    short_id: "2222222".into(),
                    change_id: String::new(),
                    description: "Bumps".into(),
                },
            ]
        );
    }

    #[test]
    fn parses_diff_summary_prefixes() {
        let raw = "M flake.lock\nA machines/saya/new.nix\nD old.nix\nR {a.nix => b.nix}\n";
        assert_eq!(
            parse_diff_summary(raw),
            vec![
                "flake.lock",
                "machines/saya/new.nix",
                "old.nix",
                "{a.nix => b.nix}",
            ]
        );
        assert!(parse_diff_summary("").is_empty());
    }

    #[test]
    fn parses_parent_mutable_bumps_cases() {
        assert!(parse_parent_mutable_bumps("1\tBumps\n"));
        assert!(parse_parent_mutable_bumps("1\t  Bumps  \n"));
        assert!(!parse_parent_mutable_bumps("0\tBumps\n"));
        assert!(!parse_parent_mutable_bumps("1\tBumps extra\n"));
        assert!(!parse_parent_mutable_bumps("1\t\n"));
        assert!(!parse_parent_mutable_bumps(""));
        // Multiple parents (merge commit) must return false.
        assert!(!parse_parent_mutable_bumps("1\tBumps\n1\tOther\n"));
    }

    #[test]
    fn parses_conflict_list_and_push_summary() {
        let raw = "abcd1234\tLocal change\nef567890\t\n";
        assert_eq!(
            parse_conflict_list(raw),
            vec!["abcd1234 (Local change)", "ef567890 (unnamed)"]
        );
        assert_eq!(format_push_summary("", ""), "pushed master");
        assert_eq!(
            format_push_summary("", "Changes to push to origin:\n  Move bookmark master\n"),
            "Changes to push to origin:; Move bookmark master"
        );
    }

    // ---- Temporary-directory integration tests ----

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(label: &str) -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let ts = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let path = std::env::temp_dir().join(format!(
                "nix-deploy-repo-test-{label}-{}-{ts}-{n}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("creating temp test dir");
            Self { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn has_tool(bin: &str) -> bool {
        Command::new(bin)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn git_ok(dir: &Path, args: &[&str]) -> String {
        run_git(dir, args).unwrap_or_else(|e| panic!("git {args:?} failed: {e}"))
    }

    fn jj_ok(dir: &Path, args: &[&str]) -> String {
        run_jj(dir, false, args).unwrap_or_else(|e| panic!("jj {args:?} failed: {e}"))
    }

    struct JjFixture {
        _tmp: TempDir,
        seed: PathBuf,
        work: PathBuf,
    }

    impl JjFixture {
        fn create(label: &str) -> Self {
            let tmp = TempDir::new(label);
            let origin = tmp.path.join("origin.git");
            let seed = tmp.path.join("seed");
            let work = tmp.path.join("work");
            fs::create_dir_all(&origin).expect("create origin dir");
            fs::create_dir_all(&seed).expect("create seed dir");

            git_ok(&origin, &["init", "--bare", "-b", "master"]);
            git_ok(&seed, &["init", "-b", "master"]);
            git_ok(&seed, &["config", "user.name", "Test User"]);
            git_ok(&seed, &["config", "user.email", "test@example.com"]);
            fs::write(seed.join("flake.lock"), "{\"version\": 7, \"rev\": 1}\n")
                .expect("write initial flake.lock");
            fs::write(seed.join("config.nix"), "{}\n").expect("write config.nix");
            git_ok(&seed, &["add", "."]);
            git_ok(&seed, &["commit", "-m", "Initial commit"]);
            git_ok(
                &seed,
                &[
                    "remote",
                    "add",
                    "origin",
                    origin.to_str().expect("utf8 path"),
                ],
            );
            git_ok(&seed, &["push", "-u", "origin", "master"]);

            let status = Command::new("jj")
                .env("JJ_USER", "Test User")
                .env("JJ_EMAIL", "test@example.com")
                .args([
                    "--no-pager",
                    "--color=never",
                    "git",
                    "clone",
                    "--colocate",
                    origin.to_str().expect("utf8 path"),
                    work.to_str().expect("utf8 path"),
                ])
                .stdin(Stdio::null())
                .output()
                .expect("spawning jj git clone");
            assert!(
                status.status.success(),
                "jj git clone failed: {}",
                String::from_utf8_lossy(&status.stderr)
            );
            git_ok(&work, &["config", "user.name", "Test User"]);
            git_ok(&work, &["config", "user.email", "test@example.com"]);
            jj_ok(
                &work,
                &["config", "set", "--repo", "user.name", "Test User"],
            );
            jj_ok(
                &work,
                &["config", "set", "--repo", "user.email", "test@example.com"],
            );

            Self {
                _tmp: tmp,
                seed,
                work,
            }
        }

        fn push_remote_commit(&self, file: &str, content: &str, message: &str) {
            fs::write(self.seed.join(file), content).expect("write remote file");
            git_ok(&self.seed, &["add", file]);
            git_ok(&self.seed, &["commit", "-m", message]);
            git_ok(&self.seed, &["push", "origin", "master"]);
            jj_ok(&self.work, &["git", "fetch"]);
        }
    }

    #[test]
    fn plain_git_repo_inspect_and_commits_between() {
        if !has_tool("git") {
            return;
        }
        let tmp = TempDir::new("plain-git");
        git_ok(&tmp.path, &["init", "-b", "master"]);
        git_ok(&tmp.path, &["config", "user.name", "Test User"]);
        git_ok(&tmp.path, &["config", "user.email", "test@example.com"]);

        fs::write(tmp.path.join("a.txt"), "one\n").unwrap();
        git_ok(&tmp.path, &["add", "."]);
        git_ok(&tmp.path, &["commit", "-m", "First"]);
        let rev1 = git_ok(&tmp.path, &["rev-parse", "HEAD"]).trim().to_string();

        fs::write(tmp.path.join("a.txt"), "two\n").unwrap();
        git_ok(&tmp.path, &["add", "."]);
        git_ok(&tmp.path, &["commit", "-m", "Second"]);
        let rev2 = git_ok(&tmp.path, &["rev-parse", "HEAD"]).trim().to_string();

        let state = inspect(&tmp.path).unwrap();
        assert!(!state.is_jj);
        assert_eq!(state, RepoState::default());

        let between = commits_between(&tmp.path, &rev1, &rev2).unwrap();
        assert_eq!(between.len(), 1);
        assert_eq!(between[0].commit_id, rev2);
        assert_eq!(between[0].description, "Second");
        assert!(between[0].change_id.is_empty());

        assert!(commits_between(&tmp.path, &rev2, &rev2).unwrap().is_empty());
    }

    #[test]
    fn jj_commit_hygiene_bumps_squash_and_rollback() {
        if !has_tool("jj") || !has_tool("git") {
            return;
        }
        let fx = JjFixture::create("hygiene");

        // Clean initial checkout.
        let s0 = inspect(&fx.work).unwrap();
        assert!(s0.is_jj);
        assert!(s0.op_id.is_some());
        assert!(!s0.wc_dirty);
        assert!(!s0.wc_only_flake_lock());
        assert!(!s0.wc_is_named());
        assert!(!s0.parent_is_mutable_bumps);
        assert!(s0.incoming_commits.is_empty());
        assert!(s0.unpushed_commits.is_empty());

        // Dirty unnamed non-flake.lock edit -> describe_and_new.
        fs::write(fx.work.join("config.nix"), "{ a = 1; }\n").unwrap();
        let s1 = inspect(&fx.work).unwrap();
        assert!(s1.wc_dirty);
        assert!(!s1.wc_only_flake_lock());
        assert!(!s1.wc_is_named());
        assert_eq!(s1.wc_changed_files, vec!["config.nix"]);

        describe_and_new(&fx.work, "Set a = 1").unwrap();
        let s2 = inspect(&fx.work).unwrap();
        assert!(!s2.wc_dirty);
        assert_eq!(s2.unpushed_commits.len(), 1);
        assert_eq!(s2.unpushed_commits[0].description, "Set a = 1");

        // Dirty already-named edit -> new_commit.
        jj_ok(&fx.work, &["describe", "-m", "Set a = 2"]);
        fs::write(fx.work.join("config.nix"), "{ a = 2; }\n").unwrap();
        let s3 = inspect(&fx.work).unwrap();
        assert!(s3.wc_dirty);
        assert!(s3.wc_is_named());
        assert_eq!(s3.wc_description, "Set a = 2");

        new_commit(&fx.work).unwrap();
        let s4 = inspect(&fx.work).unwrap();
        assert!(!s4.wc_dirty);
        assert_eq!(s4.unpushed_commits.len(), 2);

        // First flake.lock bump -> creates new Bumps commit.
        let pre_bump_op = current_op_id(&fx.work).unwrap();
        fs::write(fx.work.join("flake.lock"), "{\"version\": 7, \"rev\": 2}\n").unwrap();
        let s5 = inspect(&fx.work).unwrap();
        assert!(s5.wc_only_flake_lock());
        assert!(!s5.parent_is_mutable_bumps);

        let outcome1 = commit_flake_lock_bumps(&fx.work).unwrap();
        assert_eq!(outcome1, BumpCommitOutcome::CreatedNew);

        let s6 = inspect(&fx.work).unwrap();
        assert!(!s6.wc_dirty);
        assert!(s6.parent_is_mutable_bumps);
        assert_eq!(s6.unpushed_commits.len(), 3);
        assert_eq!(s6.unpushed_commits[0].description, "Bumps");

        // Second flake.lock bump -> squashes into mutable Bumps parent.
        fs::write(fx.work.join("flake.lock"), "{\"version\": 7, \"rev\": 3}\n").unwrap();
        let outcome2 = commit_flake_lock_bumps(&fx.work).unwrap();
        assert_eq!(outcome2, BumpCommitOutcome::SquashedIntoExisting);

        let s7 = inspect(&fx.work).unwrap();
        assert!(!s7.wc_dirty);
        assert!(s7.parent_is_mutable_bumps);
        assert_eq!(s7.unpushed_commits.len(), 3);

        // Rollback to pre_bump_op restores flake.lock and removes the Bumps commit.
        restore_op(&fx.work, &pre_bump_op).unwrap();
        let lock_after_restore = fs::read_to_string(fx.work.join("flake.lock")).unwrap();
        assert_eq!(lock_after_restore, "{\"version\": 7, \"rev\": 1}\n");
        let s8 = inspect(&fx.work).unwrap();
        assert!(!s8.wc_dirty);
        assert!(!s8.parent_is_mutable_bumps);
        assert_eq!(s8.unpushed_commits.len(), 2);

        // Push master and verify immutable Bumps parent does not get squashed into.
        fs::write(fx.work.join("flake.lock"), "{\"version\": 7, \"rev\": 4}\n").unwrap();
        assert_eq!(
            commit_flake_lock_bumps(&fx.work).unwrap(),
            BumpCommitOutcome::CreatedNew
        );
        let push_msg = push_master(&fx.work).unwrap();
        assert!(!push_msg.is_empty());

        let s9 = inspect(&fx.work).unwrap();
        assert!(s9.unpushed_commits.is_empty());
        assert!(!s9.parent_is_mutable_bumps);

        // New bump after pushing must create a new Bumps commit rather than squashing immutable @-.
        fs::write(fx.work.join("flake.lock"), "{\"version\": 7, \"rev\": 5}\n").unwrap();
        assert_eq!(
            commit_flake_lock_bumps(&fx.work).unwrap(),
            BumpCommitOutcome::CreatedNew
        );
        let s10 = inspect(&fx.work).unwrap();
        assert_eq!(s10.unpushed_commits.len(), 1);
        assert!(s10.parent_is_mutable_bumps);
    }

    #[test]
    fn jj_rebase_empty_wc_and_local_commits_and_conflict_detection() {
        if !has_tool("jj") || !has_tool("git") {
            return;
        }

        // Case 1: empty @ sitting behind trunk().
        let fx1 = JjFixture::create("rebase-empty");
        fx1.push_remote_commit("remote.nix", "{ r = 1; }\n", "Remote update");

        let before = inspect(&fx1.work).unwrap();
        assert_eq!(before.incoming_commits.len(), 1);
        assert_eq!(before.incoming_commits[0].description, "Remote update");
        assert!(before.unpushed_commits.is_empty());

        rebase_onto_trunk(&fx1.work).unwrap();
        let after = inspect(&fx1.work).unwrap();
        assert!(after.incoming_commits.is_empty());
        assert!(!after.wc_dirty);
        assert!(fx1.work.join("remote.nix").exists());

        // Case 2: local non-conflicting commit rebased onto trunk().
        let fx2 = JjFixture::create("rebase-local");
        fs::write(fx2.work.join("local.nix"), "{ l = 1; }\n").unwrap();
        describe_and_new(&fx2.work, "Local feature").unwrap();
        fx2.push_remote_commit("remote.nix", "{ r = 2; }\n", "Remote feature");

        let before2 = inspect(&fx2.work).unwrap();
        assert_eq!(before2.incoming_commits.len(), 1);
        assert_eq!(before2.unpushed_commits.len(), 1);

        rebase_onto_trunk(&fx2.work).unwrap();
        let after2 = inspect(&fx2.work).unwrap();
        assert!(after2.incoming_commits.is_empty());
        assert_eq!(after2.unpushed_commits.len(), 1);
        assert_eq!(after2.unpushed_commits[0].description, "Local feature");
        assert!(fx2.work.join("remote.nix").exists());
        assert!(fx2.work.join("local.nix").exists());

        // Case 3: conflicting rebase returns a clear error.
        let fx3 = JjFixture::create("rebase-conflict");
        fs::write(fx3.work.join("config.nix"), "{ side = \"local\"; }\n").unwrap();
        describe_and_new(&fx3.work, "Local config change").unwrap();
        fx3.push_remote_commit(
            "config.nix",
            "{ side = \"remote\"; }\n",
            "Remote config change",
        );

        let err = rebase_onto_trunk(&fx3.work).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("conflicts"),
            "expected conflict error message, got: {msg}"
        );
    }
}
