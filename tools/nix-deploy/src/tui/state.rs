//! State machine, diff view model, and scrolling/navigation helpers for the
//! interactive `ratatui` TUI.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};

use crate::activate::Mode;
use crate::diff;
use crate::flake_diff::{DiffLineKind, FlakeUpdateResult, InputChange, InputDiff};
use crate::manifest::Machine;
use crate::repo::CommitSummary;

/// Which of the two main panes currently holds keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Left,
    Right,
}

impl Pane {
    /// Toggle between the left (diffs/commits) and right (build/deploy) panes.
    pub fn toggle(self) -> Self {
        match self {
            Pane::Left => Pane::Right,
            Pane::Right => Pane::Left,
        }
    }
}

/// User action chosen for one machine during activation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeployAction {
    Switch,
    Boot,
    BootReboot,
}

impl DeployAction {
    /// Human-readable label for logs and summaries.
    pub fn label(self) -> &'static str {
        match self {
            DeployAction::Switch => "switch",
            DeployAction::Boot => "boot",
            DeployAction::BootReboot => "boot + reboot",
        }
    }

    /// Corresponding `switch-to-configuration` mode.
    pub fn mode(self) -> Mode {
        match self {
            DeployAction::Switch => Mode::Switch,
            DeployAction::Boot | DeployAction::BootReboot => Mode::Boot,
        }
    }
}

/// High-level lifecycle stage of the TUI session.
#[derive(Debug)]
pub enum Stage {
    /// Initial repository inspection (`repo::inspect`).
    InspectingRepo,
    /// `trunk()` has commits not yet in `@`; ask whether to rebase.
    RepoSyncPrompt { incoming: Vec<CommitSummary> },
    /// Running `repo::rebase_onto_trunk` in the background.
    Rebasing,
    /// Dirty unnamed jj working copy; prompt for a commit message.
    CommitMessagePrompt {
        input: String,
        cursor: usize,
        changed_files: Vec<String>,
        validation_error: Option<String>,
    },
    /// Running `lineage` checks against selected machines.
    LineageCheck,
    /// Speculative background `nix flake update` modal.
    FlakeUpdateModal(FlakeModalState),
    /// Single-evaluation `nix build` running in the background.
    Building,
    /// Computing per-machine current system, closure diff, and `nvd diff`.
    ComputingMachineDiffs,
    /// Build finished while reviewable flake diffs exist; wait for explicit approval.
    ReviewGate,
    /// Waiting for the user's `[s]witch / [b]oot / [r] boot+reboot / [q] abort` decision.
    MachinePrompt { machine_idx: usize },
    /// Streaming closure copy, activation, and optional remote reboot for one machine.
    Activating {
        machine_idx: usize,
        action: DeployAction,
    },
    /// Boot entry is set on the local machine and unpushed commits exist;
    /// prompt immediately before calling `systemctl reboot`.
    PushBeforeLocalReboot {
        machine_idx: usize,
        unpushed: Vec<CommitSummary>,
        pushing: bool,
        push_error: Option<String>,
    },
    /// All machines finished without a local reboot and unpushed commits exist.
    PushAfterDeploy {
        unpushed: Vec<CommitSummary>,
        pushing: bool,
        push_error: Option<String>,
    },
    /// User aborted or a build/deploy error occurred after a `Bumps` commit was
    /// created or squashed; ask whether to restore the pre-bump jj operation.
    ConfirmAbortRollback {
        reason: String,
        op_id: String,
        is_error: bool,
    },
}

/// Interactive state inside the speculative flake-update modal.
#[derive(Debug)]
pub enum FlakeModalState {
    /// `nix flake update` is still running against a temporary lockfile.
    Running { want_apply_when_ready: bool },
    /// Candidate lockfile diff is ready and has changes to review/apply.
    Ready { result: FlakeUpdateResult },
    /// Background `nix flake update` failed; offer to continue without updating.
    Failed { error: String },
}

/// Per-machine deploy status shown in the right pane and exit summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MachineOutcome {
    Pending,
    UpToDate,
    Activated(DeployAction),
    RebootCancelledAfterBoot,
    Failed(String),
}

impl MachineOutcome {
    /// Short ASCII status badge for the right-pane machine overview.
    pub fn badge(&self) -> &'static str {
        match self {
            MachineOutcome::Pending => "pending",
            MachineOutcome::UpToDate => "up-to-date",
            MachineOutcome::Activated(DeployAction::Switch) => "switched",
            MachineOutcome::Activated(DeployAction::Boot) => "boot-set",
            MachineOutcome::Activated(DeployAction::BootReboot) => "rebooted",
            MachineOutcome::RebootCancelledAfterBoot => "boot-set",
            MachineOutcome::Failed(_) => "FAILED",
        }
    }
}

/// Computed closure diff and deployment plan for one machine.
#[derive(Debug, Clone)]
pub struct MachinePlan {
    pub machine: Machine,
    pub new_system: String,
    pub up_to_date: bool,
    pub verdict: diff::Verdict,
    pub nvd_lines: Vec<String>,
    pub outcome: MachineOutcome,
    pub activation_logs: Vec<String>,
}

/// Visual styling category for a flattened line in the left pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineStyle {
    SectionHeader,
    InputHeader,
    FileHeader,
    HunkHeader,
    Added,
    Removed,
    Context,
    Commit,
    Badge,
    Dim,
    Warning,
}

/// A single rendered row in the scrollable left pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyledLine {
    pub text: String,
    pub style: LineStyle,
    /// Whether `n`/`N` navigation stops on this line.
    pub is_jump_target: bool,
    /// Index into `AppState::reviewable_inputs` when this line belongs to an input.
    pub input_idx: Option<usize>,
}

impl StyledLine {
    fn plain(text: impl Into<String>, style: LineStyle) -> Self {
        Self {
            text: text.into(),
            style,
            is_jump_target: false,
            input_idx: None,
        }
    }

    fn jump(text: impl Into<String>, style: LineStyle, input_idx: Option<usize>) -> Self {
        Self {
            text: text.into(),
            style,
            is_jump_target: true,
            input_idx,
        }
    }

    fn for_input(text: impl Into<String>, style: LineStyle, input_idx: usize) -> Self {
        Self {
            text: text.into(),
            style,
            is_jump_target: false,
            input_idx: Some(input_idx),
        }
    }
}

/// Local commits relative to one or more machines' running revisions.
#[derive(Debug, Clone)]
pub struct LocalCommitSection {
    pub title: String,
    pub commits: Vec<CommitSummary>,
}

/// Shared UI state owned by the TUI event loop.
pub struct AppState {
    pub stage: Stage,
    pub active_pane: Pane,
    pub tick: usize,
    pub is_jj: bool,
    pub pre_existing_dirty_flake_lock: bool,
    pub pre_bump_op_id: Option<String>,
    pub running_revs: BTreeMap<String, String>,
    pub local_commits: Vec<LocalCommitSection>,
    pub ignored_inputs: Vec<InputChange>,
    pub reviewable_inputs: Vec<InputChange>,
    pub fetched_diffs: Vec<Option<InputDiff>>,
    pub show_filtered_hunks: bool,
    pub expanded_large_inputs: BTreeSet<usize>,
    pub left_lines: Vec<StyledLine>,
    pub left_scroll: usize,
    pub right_scroll: usize,
    pub right_max_scroll: Cell<usize>,
    pub right_follow_tail: bool,
    pub status_log: Vec<String>,
    pub machine_plans: Vec<MachinePlan>,
    pub active_machine_idx: Option<usize>,
    pub diffs_approved: bool,
    pub max_diff_files: usize,
    /// Final lines printed to stderr after leaving the alternate screen.
    pub exit_summary: Vec<String>,
    /// Fatal error message (if exiting with failure) plus any detailed error lines.
    pub fatal_error: Option<String>,
    pub error_details: Vec<String>,
    /// If set, the runner executes `systemctl reboot` locally right after restoring
    /// the terminal and printing the exit summary.
    pub pending_local_reboot: Option<Machine>,
    pub should_quit: bool,
}

impl AppState {
    /// Construct initial state before repository inspection completes.
    pub fn new(max_diff_files: usize) -> Self {
        let mut state = Self {
            stage: Stage::InspectingRepo,
            active_pane: Pane::Right,
            tick: 0,
            is_jj: false,
            pre_existing_dirty_flake_lock: false,
            pre_bump_op_id: None,
            running_revs: BTreeMap::new(),
            local_commits: Vec::new(),
            ignored_inputs: Vec::new(),
            reviewable_inputs: Vec::new(),
            fetched_diffs: Vec::new(),
            show_filtered_hunks: false,
            expanded_large_inputs: BTreeSet::new(),
            left_lines: Vec::new(),
            left_scroll: 0,
            right_scroll: 0,
            right_max_scroll: Cell::new(0),
            right_follow_tail: true,
            status_log: Vec::new(),
            machine_plans: Vec::new(),
            active_machine_idx: None,
            diffs_approved: false,
            max_diff_files,
            exit_summary: Vec::new(),
            fatal_error: None,
            error_details: Vec::new(),
            pending_local_reboot: None,
            should_quit: false,
        };
        state.rebuild_left_lines();
        state
    }

    /// Append a line to the status event log.
    pub fn log_status(&mut self, msg: impl Into<String>) {
        self.status_log.push(msg.into());
        if self.status_log.len() > 200 {
            let excess = self.status_log.len() - 200;
            self.status_log.drain(..excess);
        }
    }

    /// True if there is at least one non-ignored flake input change to review.
    pub fn has_reviewable_diffs(&self) -> bool {
        !self.reviewable_inputs.is_empty()
    }

    /// Number of reviewable flake input diffs that have finished fetching.
    pub fn fetched_diff_count(&self) -> usize {
        self.fetched_diffs.iter().filter(|d| d.is_some()).count()
    }

    /// Whether activation must pause for manual diff review before switching.
    pub fn requires_diff_review_gate(&self) -> bool {
        self.has_reviewable_diffs() && !self.diffs_approved
    }

    /// Decide whether a machine can auto-switch without prompting.
    ///
    /// Auto-switch is allowed only when there are no reviewable flake input
    /// diffs AND the machine's closure diff has no reboot-worthy changes.
    pub fn can_auto_switch(&self, plan: &MachinePlan) -> bool {
        !self.has_reviewable_diffs() && !plan.verdict.reboot_recommended()
    }

    /// Register changed inputs after the user accepts a flake update.
    pub fn set_flake_changes(&mut self, changes: Vec<InputChange>) {
        self.ignored_inputs.clear();
        self.reviewable_inputs.clear();
        for change in changes {
            if change.is_ignored {
                self.ignored_inputs.push(change);
            } else {
                self.reviewable_inputs.push(change);
            }
        }
        self.fetched_diffs = vec![None; self.reviewable_inputs.len()];
        if !self.reviewable_inputs.is_empty() {
            self.active_pane = Pane::Left;
        }
        self.rebuild_left_lines();
    }

    /// Store one asynchronously fetched input diff and refresh left-pane lines.
    pub fn record_fetched_diff(&mut self, index: usize, diff: InputDiff) {
        if let Some(slot) = self.fetched_diffs.get_mut(index) {
            *slot = Some(diff);
            self.rebuild_left_lines();
        }
    }

    /// Toggle visibility of pure version/hash hunks in the left pane (`f`).
    pub fn toggle_filtered_hunks(&mut self) {
        self.show_filtered_hunks = !self.show_filtered_hunks;
        self.rebuild_left_lines();
        self.clamp_left_scroll();
    }

    /// Toggle expand/collapse for large (`collapsed_too_large`) inputs (`e`).
    ///
    /// If the current scroll offset sits inside a large input, only that input
    /// is toggled; otherwise every large input is toggled together.
    pub fn toggle_expand_large(&mut self) {
        let current_large = self
            .left_lines
            .get(self.left_scroll)
            .and_then(|line| line.input_idx)
            .filter(|&idx| {
                self.fetched_diffs
                    .get(idx)
                    .and_then(|d| d.as_ref())
                    .is_some_and(|d| d.collapsed_too_large)
            });

        if let Some(idx) = current_large {
            if !self.expanded_large_inputs.remove(&idx) {
                self.expanded_large_inputs.insert(idx);
            }
        } else {
            let all_large: Vec<usize> = self
                .fetched_diffs
                .iter()
                .enumerate()
                .filter_map(|(i, d)| d.as_ref().filter(|d| d.collapsed_too_large).map(|_| i))
                .collect();
            let all_expanded = all_large
                .iter()
                .all(|idx| self.expanded_large_inputs.contains(idx));
            if all_expanded {
                self.expanded_large_inputs.clear();
            } else {
                for idx in all_large {
                    self.expanded_large_inputs.insert(idx);
                }
            }
        }
        self.rebuild_left_lines();
        self.clamp_left_scroll();
    }

    /// Jump to the next input or file header in the left pane (`n`).
    pub fn jump_next_header(&mut self) {
        if let Some(pos) = self
            .left_lines
            .iter()
            .enumerate()
            .skip(self.left_scroll.saturating_add(1))
            .find_map(|(i, line)| line.is_jump_target.then_some(i))
        {
            self.left_scroll = pos;
        }
    }

    /// Jump to the previous input or file header in the left pane (`N`).
    pub fn jump_prev_header(&mut self) {
        if let Some(pos) = self
            .left_lines
            .iter()
            .enumerate()
            .take(self.left_scroll)
            .rev()
            .find_map(|(i, line)| line.is_jump_target.then_some(i))
        {
            self.left_scroll = pos;
        }
    }

    /// Scroll the active pane vertically by `delta` lines (positive = down).
    pub fn scroll_active(&mut self, delta: isize) {
        match self.active_pane {
            Pane::Left => {
                let next = if delta >= 0 {
                    self.left_scroll.saturating_add(delta as usize)
                } else {
                    self.left_scroll.saturating_sub(delta.unsigned_abs())
                };
                self.left_scroll = next;
                self.clamp_left_scroll();
            }
            Pane::Right => {
                let max_scroll = self.right_max_scroll.get();
                if delta < 0 {
                    let base = if self.right_follow_tail {
                        self.right_follow_tail = false;
                        max_scroll
                    } else {
                        self.right_scroll.min(max_scroll)
                    };
                    self.right_scroll = base.saturating_sub(delta.unsigned_abs());
                } else {
                    let base = if self.right_follow_tail {
                        max_scroll
                    } else {
                        self.right_scroll.min(max_scroll)
                    };
                    let next = base.saturating_add(delta as usize).min(max_scroll);
                    self.right_scroll = next;
                    if next >= max_scroll {
                        self.right_follow_tail = true;
                    }
                }
            }
        }
    }

    /// Keep `left_scroll` within the current line count.
    pub fn clamp_left_scroll(&mut self) {
        let max_scroll = self.left_lines.len().saturating_sub(1);
        if self.left_scroll > max_scroll {
            self.left_scroll = max_scroll;
        }
    }

    /// Rebuild the flattened styled lines for the left pane (`Diffs & Commits`).
    pub fn rebuild_left_lines(&mut self) {
        let mut lines = Vec::new();

        // Section 1: Local commits since running revision.
        lines.push(StyledLine::plain(
            "=== Local Commits Since Running Revision ===",
            LineStyle::SectionHeader,
        ));
        if self.local_commits.is_empty() {
            lines.push(StyledLine::plain(
                "  (none -- no local commits ahead of running systems)",
                LineStyle::Dim,
            ));
        } else {
            for section in &self.local_commits {
                if self.local_commits.len() > 1 {
                    lines.push(StyledLine::plain(
                        format!("  -- {} --", section.title),
                        LineStyle::Dim,
                    ));
                }
                if section.commits.is_empty() {
                    lines.push(StyledLine::plain("  (no commits)", LineStyle::Dim));
                } else {
                    for commit in &section.commits {
                        lines.push(StyledLine::plain(
                            format!("  {}  {}", commit.short_id, commit.description),
                            LineStyle::Commit,
                        ));
                    }
                }
            }
        }

        lines.push(StyledLine::plain("", LineStyle::Context));

        // Section 2: Flake input diffs.
        let total = self.reviewable_inputs.len();
        let fetched = self.fetched_diff_count();
        let header = if total == 0 {
            "=== Flake Input Diffs ===".to_string()
        } else if fetched < total {
            format!("=== Flake Input Diffs (Fetching {fetched}/{total}...) ===")
        } else {
            format!("=== Flake Input Diffs ({total} reviewable) ===")
        };
        lines.push(StyledLine::plain(header, LineStyle::SectionHeader));

        if total == 0 {
            lines.push(StyledLine::plain(
                "  (no non-core flake input updates to review)",
                LineStyle::Dim,
            ));
        } else {
            for (idx, change) in self.reviewable_inputs.iter().enumerate() {
                let names = change.names.join(", ");
                let rev_span = format_rev_span(change);
                lines.push(StyledLine::jump(
                    format!("==> {names} ({}) {rev_span}", change.repo_id),
                    LineStyle::InputHeader,
                    Some(idx),
                ));

                match self.fetched_diffs.get(idx).and_then(|d| d.as_ref()) {
                    None => {
                        lines.push(StyledLine::for_input(
                            "  [fetching diff...]",
                            LineStyle::Dim,
                            idx,
                        ));
                    }
                    Some(input_diff) => {
                        append_input_diff(
                            &mut lines,
                            idx,
                            input_diff,
                            self.show_filtered_hunks,
                            self.expanded_large_inputs.contains(&idx),
                            self.max_diff_files,
                        );
                    }
                }
                lines.push(StyledLine::for_input("", LineStyle::Context, idx));
            }
        }

        if !self.ignored_inputs.is_empty() {
            lines.push(StyledLine::plain(
                format!(
                    "--- Ignored core inputs updated ({}) ---",
                    self.ignored_inputs.len()
                ),
                LineStyle::Dim,
            ));
            for change in &self.ignored_inputs {
                lines.push(StyledLine::plain(
                    format!(
                        "  [core] {} ({}) {}",
                        change.names.join(", "),
                        change.repo_id,
                        format_rev_span(change)
                    ),
                    LineStyle::Dim,
                ));
            }
        }

        self.left_lines = lines;
    }
}

/// Format a short `old..new` revision span for display.
pub fn format_rev_span(change: &InputChange) -> String {
    let short = |r: &Option<String>| -> String {
        match r.as_deref() {
            Some(s) if s.len() > 10 => s[..10].to_string(),
            Some(s) => s.to_string(),
            None => "(none)".to_string(),
        }
    };
    format!("{}..{}", short(&change.old_rev), short(&change.new_rev))
}

fn append_input_diff(
    lines: &mut Vec<StyledLine>,
    idx: usize,
    diff: &InputDiff,
    show_filtered: bool,
    expanded_large: bool,
    max_diff_files: usize,
) {
    if let Some(err) = &diff.error {
        let is_empty = diff.files.is_empty() && diff.commits.is_empty();
        let prefix = if is_empty {
            "diff unavailable"
        } else {
            "partial diff"
        };
        lines.push(StyledLine::for_input(
            format!("  [warn] {prefix}: {err}"),
            LineStyle::Warning,
            idx,
        ));
        if let Some(url) = &diff.change.compare_url {
            lines.push(StyledLine::for_input(
                format!("  compare: {url}"),
                LineStyle::Dim,
                idx,
            ));
        }
        if is_empty {
            return;
        }
    }

    if !diff.commits.is_empty() {
        const MAX_SHOWN_COMMITS: usize = 15;
        lines.push(StyledLine::for_input(
            format!("  Commits ({}):", diff.commits.len()),
            LineStyle::Dim,
            idx,
        ));
        for commit in diff.commits.iter().take(MAX_SHOWN_COMMITS) {
            let short_sha = if commit.sha.len() > 8 {
                &commit.sha[..8]
            } else {
                &commit.sha
            };
            lines.push(StyledLine::for_input(
                format!("    {short_sha} {} ({})", commit.subject, commit.author),
                LineStyle::Commit,
                idx,
            ));
        }
        if diff.commits.len() > MAX_SHOWN_COMMITS {
            lines.push(StyledLine::for_input(
                format!(
                    "    ... and {} more commits",
                    diff.commits.len() - MAX_SHOWN_COMMITS
                ),
                LineStyle::Dim,
                idx,
            ));
        }
    }

    if diff.hidden_hunks > 0 {
        let badge = if show_filtered {
            format!(
                "  [f] Showing {} hash/version hunks across {} filtered files (press f to hide)",
                diff.hidden_hunks, diff.hidden_files
            )
        } else {
            format!(
                "  [f] {} hash/version hunks in {} files hidden (press f to show)",
                diff.hidden_hunks, diff.hidden_files
            )
        };
        lines.push(StyledLine::for_input(badge, LineStyle::Badge, idx));
    }

    if diff.collapsed_too_large && !expanded_large {
        lines.push(StyledLine::for_input(
            format!(
                "  [e] Collapsed: {} substantive files changed (cap {max_diff_files}); press e to expand",
                diff.substantive_files
            ),
            LineStyle::Badge,
            idx,
        ));
        if let Some(url) = &diff.change.compare_url {
            lines.push(StyledLine::for_input(
                format!("  compare: {url}"),
                LineStyle::Dim,
                idx,
            ));
        }
        return;
    }

    if diff.collapsed_too_large && expanded_large {
        lines.push(StyledLine::for_input(
            format!(
                "  [e] Expanded large diff ({} substantive files; press e to collapse)",
                diff.substantive_files
            ),
            LineStyle::Badge,
            idx,
        ));
    }

    let mut shown_any_file = false;
    for file in &diff.files {
        if file.is_filtered && !show_filtered {
            continue;
        }
        shown_any_file = true;
        let filter_tag = if file.is_filtered { " [pure bump]" } else { "" };
        lines.push(StyledLine::jump(
            format!(
                "  --- {} ({}, +{} -{}){filter_tag}",
                file.path, file.status, file.additions, file.deletions
            ),
            LineStyle::FileHeader,
            Some(idx),
        ));

        if let Some(reason) = &file.no_patch_reason {
            lines.push(StyledLine::for_input(
                format!("    ({reason})"),
                LineStyle::Dim,
                idx,
            ));
            continue;
        }

        for hunk in &file.hunks {
            if hunk.is_pure_bump && !show_filtered {
                continue;
            }
            let hunk_tag = if hunk.is_pure_bump {
                " [pure bump]"
            } else {
                ""
            };
            lines.push(StyledLine::for_input(
                format!("    {}{hunk_tag}", hunk.header),
                LineStyle::HunkHeader,
                idx,
            ));
            for dl in &hunk.lines {
                let (prefix, style) = match dl.kind {
                    DiffLineKind::Added => ("+", LineStyle::Added),
                    DiffLineKind::Removed => ("-", LineStyle::Removed),
                    DiffLineKind::Context => (" ", LineStyle::Context),
                };
                lines.push(StyledLine::for_input(
                    format!("    {prefix}{}", dl.text),
                    style,
                    idx,
                ));
            }
        }

        if !file.is_filtered && file.hidden_hunks > 0 && !show_filtered {
            lines.push(StyledLine::for_input(
                format!(
                    "    [f] {} hash/version hunk(s) in this file hidden",
                    file.hidden_hunks
                ),
                LineStyle::Badge,
                idx,
            ));
        }
    }

    if !shown_any_file {
        lines.push(StyledLine::for_input(
            "  (all changed hunks are pure version/hash bumps; press f to view)",
            LineStyle::Dim,
            idx,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flake_diff::{CommitInfo, DiffFile, DiffHunk, DiffLine, InputSource};
    use crate::target::Target;

    fn sample_change(name: &str, ignored: bool) -> InputChange {
        InputChange {
            names: vec![name.to_string()],
            source: InputSource::GitHub {
                owner: "org".into(),
                repo: name.into(),
            },
            repo_id: format!("org/{name}"),
            old_rev: Some("1111111111111111".into()),
            new_rev: Some("2222222222222222".into()),
            old_last_modified: None,
            new_last_modified: None,
            is_ignored: ignored,
            compare_url: Some(format!("https://github.com/org/{name}/compare/1111...2222")),
        }
    }

    fn sample_diff(collapsed_too_large: bool) -> InputDiff {
        InputDiff {
            change: sample_change("overlay", false),
            commits: vec![CommitInfo {
                sha: "abcdef123456".into(),
                subject: "update package".into(),
                author: "alice".into(),
            }],
            files: vec![
                DiffFile {
                    path: "versions.json".into(),
                    status: "modified".into(),
                    additions: 1,
                    deletions: 1,
                    hunks: vec![DiffHunk {
                        header: "@@ -1,1 +1,1 @@".into(),
                        lines: vec![
                            DiffLine {
                                kind: DiffLineKind::Removed,
                                text: "\"version\": \"1.0\"".into(),
                            },
                            DiffLine {
                                kind: DiffLineKind::Added,
                                text: "\"version\": \"1.1\"".into(),
                            },
                        ],
                        is_pure_bump: true,
                    }],
                    no_patch_reason: None,
                    is_filtered: true,
                    hidden_hunks: 1,
                },
                DiffFile {
                    path: "default.nix".into(),
                    status: "modified".into(),
                    additions: 1,
                    deletions: 0,
                    hunks: vec![DiffHunk {
                        header: "@@ -10,2 +10,3 @@".into(),
                        lines: vec![DiffLine {
                            kind: DiffLineKind::Added,
                            text: "postInstall = \"echo hi\";".into(),
                        }],
                        is_pure_bump: false,
                    }],
                    no_patch_reason: None,
                    is_filtered: false,
                    hidden_hunks: 0,
                },
            ],
            hidden_files: 1,
            hidden_hunks: 1,
            substantive_files: if collapsed_too_large { 75 } else { 1 },
            collapsed_too_large,
            error: None,
        }
    }

    #[test]
    fn filtered_hunks_hidden_by_default_and_toggled_with_f() {
        let mut app = AppState::new(60);
        app.set_flake_changes(vec![sample_change("overlay", false)]);
        app.record_fetched_diff(0, sample_diff(false));

        assert!(!app
            .left_lines
            .iter()
            .any(|l| l.text.contains("versions.json")));
        assert!(app
            .left_lines
            .iter()
            .any(|l| l.text.contains("default.nix")));
        assert!(app
            .left_lines
            .iter()
            .any(|l| l.text.contains("[f] 1 hash/version hunks")));

        app.toggle_filtered_hunks();
        assert!(app
            .left_lines
            .iter()
            .any(|l| l.text.contains("versions.json")));
        assert!(app
            .left_lines
            .iter()
            .any(|l| l.text.contains("[pure bump]")));
    }

    #[test]
    fn large_diff_starts_collapsed_and_toggles_with_e() {
        let mut app = AppState::new(60);
        app.set_flake_changes(vec![sample_change("overlay", false)]);
        app.record_fetched_diff(0, sample_diff(true));

        assert!(app
            .left_lines
            .iter()
            .any(|l| l.text.contains("[e] Collapsed: 75")));
        assert!(!app
            .left_lines
            .iter()
            .any(|l| l.text.contains("default.nix")));

        app.toggle_expand_large();
        assert!(app
            .left_lines
            .iter()
            .any(|l| l.text.contains("default.nix")));

        app.toggle_expand_large();
        assert!(!app
            .left_lines
            .iter()
            .any(|l| l.text.contains("default.nix")));
    }

    #[test]
    fn activation_gating_blocks_auto_switch_when_reviewable_diffs_exist() {
        let machine = Machine {
            name: "tsugumi".into(),
            target: Target::Local { sudo: true },
        };
        let plan = MachinePlan {
            machine,
            new_system: "/nix/store/new".into(),
            up_to_date: false,
            verdict: diff::Verdict::default(),
            nvd_lines: vec![],
            outcome: MachineOutcome::Pending,
            activation_logs: vec![],
        };

        let mut app = AppState::new(60);
        assert!(app.can_auto_switch(&plan));
        assert!(!app.requires_diff_review_gate());

        app.set_flake_changes(vec![sample_change("overlay", false)]);
        assert!(!app.can_auto_switch(&plan));
        assert!(app.requires_diff_review_gate());
    }

    #[test]
    fn jump_navigation_moves_between_headers() {
        let mut app = AppState::new(60);
        app.set_flake_changes(vec![sample_change("overlay", false)]);
        app.record_fetched_diff(0, sample_diff(false));

        let targets: Vec<usize> = app
            .left_lines
            .iter()
            .enumerate()
            .filter_map(|(i, l)| l.is_jump_target.then_some(i))
            .collect();
        assert!(targets.len() >= 2);

        app.left_scroll = 0;
        app.jump_next_header();
        assert_eq!(app.left_scroll, targets[0]);
        app.jump_next_header();
        assert_eq!(app.left_scroll, targets[1]);
        app.jump_prev_header();
        assert_eq!(app.left_scroll, targets[0]);
    }

    #[test]
    fn partial_diff_with_error_still_renders_commits_and_files() {
        let mut app = AppState::new(60);
        app.set_flake_changes(vec![sample_change("overlay", false)]);

        let mut truncated = sample_diff(false);
        truncated.error = Some("GitHub compare API truncated at 300 files".into());
        app.record_fetched_diff(0, truncated);

        assert!(app
            .left_lines
            .iter()
            .any(|l| l.text.contains("[warn] partial diff:")));
        assert!(app.left_lines.iter().any(|l| l.text.contains("compare:")));
        assert!(app
            .left_lines
            .iter()
            .any(|l| l.text.contains("update package")));
        assert!(app
            .left_lines
            .iter()
            .any(|l| l.text.contains("default.nix")));

        let empty_err = InputDiff {
            change: sample_change("overlay", false),
            commits: vec![],
            files: vec![],
            hidden_files: 0,
            hidden_hunks: 0,
            substantive_files: 0,
            collapsed_too_large: false,
            error: Some("network error".into()),
        };
        app.record_fetched_diff(0, empty_err);
        assert!(app
            .left_lines
            .iter()
            .any(|l| l.text.contains("[warn] diff unavailable: network error")));
        assert!(!app
            .left_lines
            .iter()
            .any(|l| l.text.contains("default.nix")));
    }

    #[test]
    fn right_pane_scroll_clamps_and_leaves_follow_tail_from_bottom() {
        let mut app = AppState::new(60);
        app.active_pane = Pane::Right;
        app.right_follow_tail = true;
        app.right_scroll = 0;
        app.right_max_scroll.set(25);

        // Scrolling up from follow-tail starts at max_scroll, not 0.
        app.scroll_active(-1);
        assert!(!app.right_follow_tail);
        assert_eq!(app.right_scroll, 24);

        app.scroll_active(-5);
        assert_eq!(app.right_scroll, 19);

        // Scrolling down clamps at max_scroll and re-enables follow-tail.
        app.scroll_active(50);
        assert_eq!(app.right_scroll, 25);
        assert!(app.right_follow_tail);

        // Scrolling up again immediately responds by moving to 24 (never stuck).
        app.scroll_active(-1);
        assert_eq!(app.right_scroll, 24);
        assert!(!app.right_follow_tail);
    }
}
