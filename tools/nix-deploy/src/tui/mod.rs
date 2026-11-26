//! Interactive `ratatui` TUI driver, terminal RAII safety guard, background
//! task coordination, and keyboard event loop.

mod render;
mod state;

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use crossterm::cursor::Show;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::activate;
use crate::build::BuildHandle;
use crate::diff;
use crate::flake_diff::{
    DiffConfig, FlakeUpdateResult, InputChange, InputDiff, PendingFlakeUpdate,
};
use crate::lineage;
use crate::log_model::Model;
use crate::manifest::{Machine, Settings};
use crate::repo::{self, BumpCommitOutcome, RepoState};
use crate::sysmon::SysMon;
use crate::tui::state::{
    AppState, DeployAction, FlakeModalState, LocalCommitSection, MachineOutcome, MachinePlan, Pane,
    Stage,
};

const POLL_INTERVAL: Duration = Duration::from_millis(50);
const SYSMON_INTERVAL: Duration = Duration::from_millis(500);
const MAX_DIFF_FETCH_WORKERS: usize = 4;

/// Global flag so the panic hook only restores the terminal while the TUI owns it.
static TERMINAL_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Options passed from `main` into the interactive TUI session.
pub struct TuiConfig {
    pub root: PathBuf,
    pub machines: Vec<Machine>,
    pub settings: Settings,
    pub strict: bool,
    pub allow_rollback: bool,
}

/// RAII guard that enters raw mode + alternate screen on construction and
/// always restores the terminal on drop or panic.
struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<io::Stderr>>,
}

impl TerminalGuard {
    fn enter() -> Result<Self> {
        install_panic_hook_once();
        enable_raw_mode().context("enabling terminal raw mode")?;
        let mut stderr = io::stderr();
        if let Err(err) = stderr.execute(EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(err).context("entering alternate screen");
        }
        TERMINAL_ACTIVE.store(true, Ordering::SeqCst);
        let backend = CrosstermBackend::new(io::stderr());
        let mut terminal = match Terminal::new(backend) {
            Ok(t) => t,
            Err(err) => {
                restore_terminal();
                return Err(err).context("creating ratatui terminal");
            }
        };
        let _ = terminal.hide_cursor();
        let _ = terminal.clear();
        Ok(Self { terminal })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = self.terminal.show_cursor();
        restore_terminal();
    }
}

fn restore_terminal() {
    if TERMINAL_ACTIVE.swap(false, Ordering::SeqCst) {
        let _ = disable_raw_mode();
        let mut stderr = io::stderr();
        let _ = stderr.execute(Show);
        let _ = stderr.execute(LeaveAlternateScreen);
        let _ = stderr.flush();
    }
}

fn install_panic_hook_once() {
    static INSTALLED: std::sync::Once = std::sync::Once::new();
    INSTALLED.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore_terminal();
            prev(info);
        }));
    });
}

/// Messages sent from background worker threads back into the UI event loop.
enum WorkerMsg {
    RepoInspected {
        after_rebase: bool,
        result: Result<RepoState>,
    },
    RebaseDone(Result<()>),
    LineageDone(Result<LineageSummary>),
    LocalCommitsLoaded(Vec<LocalCommitSection>),
    InputDiffFetched {
        index: usize,
        diff: InputDiff,
    },
    MachineDiffsDone(Result<Vec<MachinePlan>>),
    ActivationLine {
        machine_idx: usize,
        line: String,
    },
    ActivationDone {
        machine_idx: usize,
        action: DeployAction,
        result: Result<()>,
    },
    PostDeployInspect(Result<RepoState>),
    PreRebootInspect {
        machine_idx: usize,
        result: Result<RepoState>,
    },
    PushDone {
        for_local_reboot: Option<usize>,
        result: Result<String>,
    },
}

struct LineageSummary {
    warnings: Vec<String>,
    refusals: Vec<String>,
    running_revs: BTreeMap<String, String>,
}

/// Active child-process handles owned by the runner; dropping `TuiRuntime`
/// cancels and reaps any in-flight `nix flake update` or `nix build`.
struct TuiRuntime {
    cfg: TuiConfig,
    diff_cfg: DiffConfig,
    app: AppState,
    sysmon: SysMon,
    last_sysmon: Instant,
    build_model: Arc<Mutex<Model>>,
    pending_flake: Option<PendingFlakeUpdate>,
    build_handle: Option<BuildHandle>,
    worker_tx: mpsc::Sender<WorkerMsg>,
    worker_rx: Receiver<WorkerMsg>,
}

impl TuiRuntime {
    fn new(cfg: TuiConfig) -> Self {
        let diff_cfg = DiffConfig {
            ignore_repos: cfg.settings.ignore_diff_repos.clone(),
            max_diff_files: cfg.settings.max_diff_files,
        };
        let app = AppState::new(cfg.settings.max_diff_files);
        let mut sysmon = SysMon::new();
        let _ = sysmon.sample();
        let (worker_tx, worker_rx) = mpsc::channel();
        Self {
            cfg,
            diff_cfg,
            app,
            sysmon,
            last_sysmon: Instant::now(),
            build_model: Arc::new(Mutex::new(Model::new())),
            pending_flake: None,
            build_handle: None,
            worker_tx,
            worker_rx,
        }
    }

    fn spawn_repo_inspect(&self, after_rebase: bool) {
        let root = self.cfg.root.clone();
        let tx = self.worker_tx.clone();
        std::thread::spawn(move || {
            let result = repo::inspect(&root);
            let _ = tx.send(WorkerMsg::RepoInspected {
                after_rebase,
                result,
            });
        });
    }

    fn spawn_rebase(&mut self) {
        self.app.stage = Stage::Rebasing;
        let root = self.cfg.root.clone();
        let tx = self.worker_tx.clone();
        std::thread::spawn(move || {
            let result = repo::rebase_onto_trunk(&root);
            let _ = tx.send(WorkerMsg::RebaseDone(result));
        });
    }

    fn spawn_lineage_check(&mut self) {
        self.app.stage = Stage::LineageCheck;
        let root = self.cfg.root.clone();
        let machines = self.cfg.machines.clone();
        let strict = self.cfg.strict;
        let allow_rollback = self.cfg.allow_rollback;
        let tx = self.worker_tx.clone();
        std::thread::spawn(move || {
            let result = run_lineage_check(&root, &machines, strict, allow_rollback);
            let _ = tx.send(WorkerMsg::LineageDone(result));
        });
    }

    fn begin_flake_update(&mut self) {
        match PendingFlakeUpdate::spawn(&self.cfg.root, self.diff_cfg.clone()) {
            Ok(pending) => {
                self.pending_flake = Some(pending);
                self.app.stage = Stage::FlakeUpdateModal(FlakeModalState::Running {
                    want_apply_when_ready: false,
                });
            }
            Err(err) => {
                self.app
                    .log_status(format!("warning: could not start flake update: {err}"));
                self.app.stage = Stage::FlakeUpdateModal(FlakeModalState::Failed {
                    error: format!("{err:#}"),
                });
            }
        }
    }

    fn handle_repo_state(&mut self, state: RepoState, after_rebase: bool) {
        self.app.is_jj = state.is_jj;
        if !state.is_jj {
            self.spawn_lineage_check();
            return;
        }
        if !after_rebase && !state.incoming_commits.is_empty() {
            self.app.stage = Stage::RepoSyncPrompt {
                incoming: state.incoming_commits,
            };
            return;
        }
        if state.wc_dirty {
            if state.wc_only_flake_lock() {
                self.app.pre_existing_dirty_flake_lock = true;
                self.app.log_status(
                    "Working copy has only flake.lock changes; deferring to Bumps step",
                );
                self.spawn_lineage_check();
            } else if !state.wc_is_named() {
                self.app.stage = Stage::CommitMessagePrompt {
                    input: String::new(),
                    cursor: 0,
                    changed_files: state.wc_changed_files,
                    validation_error: None,
                };
            } else {
                match repo::new_commit(&self.cfg.root) {
                    Ok(()) => {
                        self.app
                            .log_status("Created new jj commit on top of named working copy");
                        self.spawn_lineage_check();
                    }
                    Err(err) => {
                        self.fail_immediately(format!("creating jj commit failed: {err:#}"))
                    }
                }
            }
        } else {
            self.spawn_lineage_check();
        }
    }

    fn start_build(&mut self, accepted: Option<FlakeUpdateResult>) {
        if let Some(pending) = self.pending_flake.take() {
            pending.cancel();
        }

        if let Some(update) = accepted {
            if self.app.is_jj {
                match repo::current_op_id(&self.cfg.root) {
                    Ok(op_id) => self.app.pre_bump_op_id = Some(op_id),
                    Err(err) => self
                        .app
                        .log_status(format!("warning: could not record pre-bump jj op: {err}")),
                }
            }
            if let Err(err) = update.apply(&self.cfg.root) {
                self.fail_or_rollback(format!("applying updated flake.lock failed: {err:#}"));
                return;
            }
            if self.app.is_jj {
                match repo::commit_flake_lock_bumps(&self.cfg.root) {
                    Ok(BumpCommitOutcome::SquashedIntoExisting) => {
                        self.app
                            .log_status("Squashed flake.lock update into existing Bumps commit");
                    }
                    Ok(BumpCommitOutcome::CreatedNew) => {
                        self.app
                            .log_status("Created new Bumps commit for flake.lock");
                    }
                    Err(err) => {
                        self.fail_or_rollback(format!(
                            "committing flake.lock Bumps failed: {err:#}"
                        ));
                        return;
                    }
                }
            }
            let reviewable: Vec<InputChange> =
                update.reviewable_changes().into_iter().cloned().collect();
            self.app.set_flake_changes(update.changes);
            if !reviewable.is_empty() {
                self.spawn_diff_fetchers(reviewable);
            }
        } else if self.app.is_jj && self.app.pre_existing_dirty_flake_lock {
            match repo::current_op_id(&self.cfg.root) {
                Ok(op_id) => self.app.pre_bump_op_id = Some(op_id),
                Err(err) => self
                    .app
                    .log_status(format!("warning: could not record pre-bump jj op: {err}")),
            }
            match repo::commit_flake_lock_bumps(&self.cfg.root) {
                Ok(BumpCommitOutcome::SquashedIntoExisting) => {
                    self.app
                        .log_status("Squashed dirty flake.lock into existing Bumps commit");
                }
                Ok(BumpCommitOutcome::CreatedNew) => {
                    self.app.log_status("Committed dirty flake.lock as Bumps");
                }
                Err(err) => {
                    self.fail_or_rollback(format!("committing flake.lock Bumps failed: {err:#}"));
                    return;
                }
            }
        }

        self.spawn_local_commits_loader();

        self.build_model = Arc::new(Mutex::new(Model::new()));
        let names: Vec<&str> = self.cfg.machines.iter().map(|m| m.name.as_str()).collect();
        self.app.log_status(format!(
            "Building {} (single evaluation) ...",
            names.join(", ")
        ));

        match BuildHandle::spawn(&self.cfg.machines, Arc::clone(&self.build_model)) {
            Ok(handle) => {
                self.build_handle = Some(handle);
                self.app.stage = Stage::Building;
            }
            Err(err) => self.fail_or_rollback(format!("starting nix build failed: {err:#}")),
        }
    }

    fn spawn_local_commits_loader(&self) {
        let root = self.cfg.root.clone();
        let running_revs = self.app.running_revs.clone();
        let tx = self.worker_tx.clone();
        std::thread::spawn(move || {
            let Ok(deploying) = lineage::deploying_rev(&root) else {
                return;
            };
            let mut by_old: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for (machine, old_rev) in running_revs {
                by_old.entry(old_rev).or_default().push(machine);
            }
            let mut sections = Vec::new();
            for (old_rev, machines) in by_old {
                if old_rev == deploying.commit {
                    continue;
                }
                if let Ok(commits) = repo::commits_between(&root, &old_rev, &deploying.commit) {
                    if !commits.is_empty() {
                        let short = &old_rev[..old_rev.len().min(10)];
                        sections.push(LocalCommitSection {
                            title: format!("{} (since {short})", machines.join(", ")),
                            commits,
                        });
                    }
                }
            }
            let _ = tx.send(WorkerMsg::LocalCommitsLoaded(sections));
        });
    }

    fn spawn_diff_fetchers(&self, reviewable: Vec<InputChange>) {
        let cfg = self.diff_cfg.clone();
        let tx = self.worker_tx.clone();
        let work_queue = Arc::new(Mutex::new(
            reviewable
                .into_iter()
                .enumerate()
                .collect::<std::collections::VecDeque<(usize, InputChange)>>(),
        ));
        let workers =
            MAX_DIFF_FETCH_WORKERS.min(work_queue.lock().map(|q| q.len()).unwrap_or(1).max(1));
        for _ in 0..workers {
            let queue = Arc::clone(&work_queue);
            let cfg = cfg.clone();
            let tx = tx.clone();
            std::thread::spawn(move || loop {
                let next = queue.lock().ok().and_then(|mut q| q.pop_front());
                let Some((index, change)) = next else {
                    break;
                };
                let diff = crate::flake_diff::fetch_one_diff(&change, &cfg);
                if tx
                    .send(WorkerMsg::InputDiffFetched { index, diff })
                    .is_err()
                {
                    break;
                }
            });
        }
    }

    fn spawn_machine_diffs(&mut self, toplevels: BTreeMap<String, String>) {
        self.app.stage = Stage::ComputingMachineDiffs;
        let machines = self.cfg.machines.clone();
        let tx = self.worker_tx.clone();
        std::thread::spawn(move || {
            let result = compute_machine_plans(&machines, &toplevels);
            let _ = tx.send(WorkerMsg::MachineDiffsDone(result));
        });
    }

    fn advance_machine_flow(&mut self, start_idx: usize) {
        let mut idx = start_idx;
        while idx < self.app.machine_plans.len() {
            if self.app.machine_plans[idx].up_to_date {
                let name = self.app.machine_plans[idx].machine.name.clone();
                self.app.machine_plans[idx].outcome = MachineOutcome::UpToDate;
                self.app.log_status(format!("{name}: already up to date"));
                idx += 1;
                continue;
            }
            self.app.active_machine_idx = Some(idx);
            let can_auto = self.app.can_auto_switch(&self.app.machine_plans[idx]);
            if can_auto {
                let name = self.app.machine_plans[idx].machine.name.clone();
                self.app
                    .log_status(format!("{name}: no reboot-worthy changes; auto-switching"));
                self.spawn_activation(idx, DeployAction::Switch);
            } else {
                self.app.right_follow_tail = false;
                self.app.right_scroll = 0;
                self.app.stage = Stage::MachinePrompt { machine_idx: idx };
            }
            return;
        }

        self.app.active_machine_idx = None;
        self.app.log_status("All selected machines deployed");
        if self.app.is_jj {
            let root = self.cfg.root.clone();
            let tx = self.worker_tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(WorkerMsg::PostDeployInspect(repo::inspect(&root)));
            });
        } else {
            self.finish_cleanly();
        }
    }

    fn spawn_activation(&mut self, machine_idx: usize, action: DeployAction) {
        let plan = &self.app.machine_plans[machine_idx];
        let machine = plan.machine.clone();
        let new_system = plan.new_system.clone();
        let reboot_timeout = Duration::from_secs(self.cfg.settings.reboot_timeout_secs);
        let tx = self.worker_tx.clone();

        self.app.right_follow_tail = true;
        self.app.stage = Stage::Activating {
            machine_idx,
            action,
        };

        std::thread::spawn(move || {
            let send = |line: String| {
                let _ = tx.send(WorkerMsg::ActivationLine { machine_idx, line });
            };
            let result = run_activation(&machine, &new_system, action, reboot_timeout, send);
            let _ = tx.send(WorkerMsg::ActivationDone {
                machine_idx,
                action,
                result,
            });
        });
    }

    fn spawn_push(&mut self, for_local_reboot: Option<usize>) {
        let root = self.cfg.root.clone();
        let tx = self.worker_tx.clone();
        std::thread::spawn(move || {
            let result = repo::push_master(&root);
            let _ = tx.send(WorkerMsg::PushDone {
                for_local_reboot,
                result,
            });
        });
    }

    fn request_abort(&mut self, reason: impl Into<String>) {
        let reason = reason.into();
        if let Some(pending) = self.pending_flake.take() {
            pending.cancel();
        }
        if let Some(handle) = self.build_handle.take() {
            handle.cancel();
        }
        if let Some(op_id) = self.app.pre_bump_op_id.clone() {
            self.app.stage = Stage::ConfirmAbortRollback {
                reason,
                op_id,
                is_error: false,
            };
        } else {
            self.app.exit_summary.push(reason);
            self.app.should_quit = true;
        }
    }

    fn fail_or_rollback(&mut self, reason: String) {
        if let Some(pending) = self.pending_flake.take() {
            pending.cancel();
        }
        if let Some(handle) = self.build_handle.take() {
            handle.cancel();
        }
        self.app.fatal_error = Some(reason.clone());
        if let Some(op_id) = self.app.pre_bump_op_id.clone() {
            self.app.stage = Stage::ConfirmAbortRollback {
                reason,
                op_id,
                is_error: true,
            };
        } else {
            self.app.should_quit = true;
        }
    }

    fn fail_immediately(&mut self, reason: String) {
        if let Some(pending) = self.pending_flake.take() {
            pending.cancel();
        }
        if let Some(handle) = self.build_handle.take() {
            handle.cancel();
        }
        self.app.fatal_error = Some(reason);
        self.app.should_quit = true;
    }

    fn finish_cleanly(&mut self) {
        for plan in &self.app.machine_plans {
            self.app
                .exit_summary
                .push(format!("{}: {}", plan.machine.name, plan.outcome.badge()));
        }
        self.app.should_quit = true;
    }

    fn poll_background(&mut self) {
        // Sample system monitor every ~500ms.
        if self.last_sysmon.elapsed() >= SYSMON_INTERVAL {
            let _ = self.sysmon.sample();
            self.last_sysmon = Instant::now();
        }

        // Poll speculative nix flake update if in flight.
        if let Some(pending) = self.pending_flake.as_mut() {
            if let Some(res) = pending.try_take() {
                self.pending_flake = None;
                let want_apply = matches!(
                    self.app.stage,
                    Stage::FlakeUpdateModal(FlakeModalState::Running {
                        want_apply_when_ready: true
                    })
                );
                match res {
                    Ok(update) => {
                        if !update.has_changes() {
                            self.app.log_status("flake.lock is already up to date");
                            self.start_build(None);
                        } else if want_apply {
                            self.start_build(Some(update));
                        } else {
                            self.app.stage =
                                Stage::FlakeUpdateModal(FlakeModalState::Ready { result: update });
                        }
                    }
                    Err(err) => {
                        self.app.stage = Stage::FlakeUpdateModal(FlakeModalState::Failed {
                            error: format!("{err:#}"),
                        });
                    }
                }
            }
        }

        // Poll active nix build.
        if let Some(handle) = self.build_handle.as_ref() {
            if let Some(res) = handle.try_take() {
                self.build_handle = None;
                match res {
                    Ok(toplevels) => {
                        let done = self.build_model.lock().map(|m| m.builds_done).unwrap_or(0);
                        self.app
                            .log_status(format!("Build finished ({done} derivations built)"));
                        self.spawn_machine_diffs(toplevels);
                    }
                    Err(err) => {
                        if let Ok(model) = self.build_model.lock() {
                            self.app.error_details.extend(model.errors.clone());
                        }
                        self.fail_or_rollback(format!("{err:#}"));
                    }
                }
            }
        }

        // Drain worker channel messages.
        while let Ok(msg) = self.worker_rx.try_recv() {
            self.handle_worker_msg(msg);
        }
    }

    fn handle_worker_msg(&mut self, msg: WorkerMsg) {
        match msg {
            WorkerMsg::RepoInspected {
                after_rebase,
                result,
            } => match result {
                Ok(state) => self.handle_repo_state(state, after_rebase),
                Err(err) => self.fail_immediately(format!("inspecting repository failed: {err:#}")),
            },
            WorkerMsg::RebaseDone(result) => match result {
                Ok(()) => {
                    self.app.log_status("Rebased onto trunk");
                    self.spawn_repo_inspect(true);
                }
                Err(err) => self.fail_immediately(format!("rebase onto trunk failed: {err:#}")),
            },
            WorkerMsg::LineageDone(result) => match result {
                Ok(summary) => {
                    for w in summary.warnings {
                        self.app.log_status(format!("warning: {w}"));
                    }
                    self.app.running_revs = summary.running_revs;
                    if !summary.refusals.is_empty() {
                        self.app.error_details = summary.refusals;
                        self.fail_immediately(
                            "refusing to deploy (--allow-rollback overrides)".to_string(),
                        );
                    } else {
                        self.begin_flake_update();
                    }
                }
                Err(err) => self.fail_immediately(format!("lineage check failed: {err:#}")),
            },
            WorkerMsg::LocalCommitsLoaded(sections) => {
                self.app.local_commits = sections;
                self.app.rebuild_left_lines();
            }
            WorkerMsg::InputDiffFetched { index, diff } => {
                self.app.record_fetched_diff(index, diff);
            }
            WorkerMsg::MachineDiffsDone(result) => match result {
                Ok(plans) => {
                    let all_up_to_date = plans.iter().all(|p| p.up_to_date);
                    self.app.machine_plans = plans;
                    if all_up_to_date {
                        for plan in &mut self.app.machine_plans {
                            plan.outcome = MachineOutcome::UpToDate;
                        }
                        self.app
                            .log_status("All selected machines are already up to date");
                        if self.app.is_jj {
                            let root = self.cfg.root.clone();
                            let tx = self.worker_tx.clone();
                            std::thread::spawn(move || {
                                let _ = tx.send(WorkerMsg::PostDeployInspect(repo::inspect(&root)));
                            });
                        } else {
                            self.finish_cleanly();
                        }
                    } else if self.app.requires_diff_review_gate() {
                        self.app.stage = Stage::ReviewGate;
                    } else {
                        self.advance_machine_flow(0);
                    }
                }
                Err(err) => {
                    self.fail_or_rollback(format!("computing closure diffs failed: {err:#}"))
                }
            },
            WorkerMsg::ActivationLine { machine_idx, line } => {
                if let Some(plan) = self.app.machine_plans.get_mut(machine_idx) {
                    plan.activation_logs.push(line);
                }
            }
            WorkerMsg::ActivationDone {
                machine_idx,
                action,
                result,
            } => match result {
                Ok(()) => {
                    let is_local_reboot = action == DeployAction::BootReboot
                        && self.app.machine_plans[machine_idx]
                            .machine
                            .target
                            .is_local();
                    if is_local_reboot {
                        if self.app.is_jj {
                            let root = self.cfg.root.clone();
                            let tx = self.worker_tx.clone();
                            std::thread::spawn(move || {
                                let _ = tx.send(WorkerMsg::PreRebootInspect {
                                    machine_idx,
                                    result: repo::inspect(&root),
                                });
                            });
                        } else {
                            let machine = self.app.machine_plans[machine_idx].machine.clone();
                            self.app.machine_plans[machine_idx].outcome =
                                MachineOutcome::Activated(DeployAction::BootReboot);
                            self.app.pending_local_reboot = Some(machine);
                            self.finish_cleanly();
                        }
                    } else {
                        let name = self.app.machine_plans[machine_idx].machine.name.clone();
                        self.app.machine_plans[machine_idx].outcome =
                            MachineOutcome::Activated(action);
                        self.app
                            .log_status(format!("{name}: deployed ({})", action.label()));
                        self.advance_machine_flow(machine_idx + 1);
                    }
                }
                Err(err) => {
                    let name = self.app.machine_plans[machine_idx].machine.name.clone();
                    self.app.machine_plans[machine_idx].outcome =
                        MachineOutcome::Failed(format!("{err:#}"));
                    self.app.error_details.extend(
                        self.app.machine_plans[machine_idx]
                            .activation_logs
                            .iter()
                            .rev()
                            .take(20)
                            .rev()
                            .cloned(),
                    );
                    self.fail_or_rollback(format!("deploying {name} failed: {err:#}"));
                }
            },
            WorkerMsg::PreRebootInspect {
                machine_idx,
                result,
            } => match result {
                Ok(state) if state.is_jj && !state.unpushed_commits.is_empty() => {
                    self.app.stage = Stage::PushBeforeLocalReboot {
                        machine_idx,
                        unpushed: state.unpushed_commits,
                        pushing: false,
                        push_error: None,
                    };
                }
                Ok(_) => {
                    let machine = self.app.machine_plans[machine_idx].machine.clone();
                    self.app.machine_plans[machine_idx].outcome =
                        MachineOutcome::Activated(DeployAction::BootReboot);
                    self.app.pending_local_reboot = Some(machine);
                    self.finish_cleanly();
                }
                Err(err) => {
                    self.app.stage = Stage::PushBeforeLocalReboot {
                        machine_idx,
                        unpushed: Vec::new(),
                        pushing: false,
                        push_error: Some(format!("could not inspect unpushed commits: {err:#}")),
                    };
                }
            },
            WorkerMsg::PostDeployInspect(result) => match result {
                Ok(state) if state.is_jj && !state.unpushed_commits.is_empty() => {
                    self.app.stage = Stage::PushAfterDeploy {
                        unpushed: state.unpushed_commits,
                        pushing: false,
                        push_error: None,
                    };
                }
                Ok(_) => self.finish_cleanly(),
                Err(err) => {
                    self.app
                        .exit_summary
                        .push(format!("warning: could not check unpushed commits: {err}"));
                    self.finish_cleanly();
                }
            },
            WorkerMsg::PushDone {
                for_local_reboot,
                result,
            } => match (for_local_reboot, result) {
                (Some(machine_idx), Ok(summary)) => {
                    self.app
                        .exit_summary
                        .push(format!("pushed master: {summary}"));
                    let machine = self.app.machine_plans[machine_idx].machine.clone();
                    self.app.machine_plans[machine_idx].outcome =
                        MachineOutcome::Activated(DeployAction::BootReboot);
                    self.app.pending_local_reboot = Some(machine);
                    self.finish_cleanly();
                }
                (Some(_), Err(err)) => {
                    if let Stage::PushBeforeLocalReboot {
                        pushing,
                        push_error,
                        ..
                    } = &mut self.app.stage
                    {
                        *pushing = false;
                        *push_error = Some(format!("{err:#}"));
                    }
                }
                (None, Ok(summary)) => {
                    self.app
                        .exit_summary
                        .push(format!("pushed master: {summary}"));
                    self.finish_cleanly();
                }
                (None, Err(err)) => {
                    if let Stage::PushAfterDeploy {
                        pushing,
                        push_error,
                        ..
                    } = &mut self.app.stage
                    {
                        *pushing = false;
                        *push_error = Some(format!("{err:#}"));
                    }
                }
            },
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }

        // Ctrl-C always aborts cleanly (or force-exits from the rollback modal).
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            if matches!(self.app.stage, Stage::ConfirmAbortRollback { .. }) {
                self.app
                    .exit_summary
                    .push("Kept Bumps commit in jj history".to_string());
                self.app.should_quit = true;
            } else {
                self.request_abort("Aborted by user (Ctrl-C)");
            }
            return;
        }

        match &mut self.app.stage {
            Stage::RepoSyncPrompt { .. } => match key.code {
                KeyCode::Char('r') => self.spawn_rebase(),
                KeyCode::Char('c') => {
                    self.app
                        .log_status("Continuing without rebasing onto trunk");
                    self.spawn_repo_inspect(true);
                }
                KeyCode::Char('q') => self.request_abort("Aborted at repo sync prompt"),
                _ => {}
            },
            Stage::CommitMessagePrompt {
                input,
                cursor,
                validation_error,
                ..
            } => match key.code {
                KeyCode::Esc => self.request_abort("Aborted at commit message prompt"),
                KeyCode::Enter => {
                    let msg = input.trim().to_string();
                    if msg.is_empty() {
                        *validation_error = Some("Commit message cannot be empty.".to_string());
                    } else {
                        match repo::describe_and_new(&self.cfg.root, &msg) {
                            Ok(()) => {
                                self.app
                                    .log_status(format!("Committed working copy: {msg}"));
                                self.spawn_lineage_check();
                            }
                            Err(err) => self.fail_immediately(format!(
                                "committing working copy failed: {err:#}"
                            )),
                        }
                    }
                }
                KeyCode::Backspace => {
                    if *cursor > 0 {
                        let prev = prev_char_boundary(input, *cursor);
                        input.drain(prev..*cursor);
                        *cursor = prev;
                        *validation_error = None;
                    }
                }
                KeyCode::Delete => {
                    if *cursor < input.len() {
                        let next = next_char_boundary(input, *cursor);
                        input.drain(*cursor..next);
                        *validation_error = None;
                    }
                }
                KeyCode::Left => *cursor = prev_char_boundary(input, *cursor),
                KeyCode::Right => *cursor = next_char_boundary(input, *cursor),
                KeyCode::Home => *cursor = 0,
                KeyCode::End => *cursor = input.len(),
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.insert(*cursor, c);
                    *cursor += c.len_utf8();
                    *validation_error = None;
                }
                _ => {}
            },
            Stage::FlakeUpdateModal(modal) => match modal {
                FlakeModalState::Running {
                    want_apply_when_ready,
                } => match key.code {
                    KeyCode::Char('y') => {
                        *want_apply_when_ready = true;
                    }
                    KeyCode::Char('n') => {
                        self.app.log_status("Skipped flake update");
                        self.start_build(None);
                    }
                    KeyCode::Char('q') => self.request_abort("Aborted at flake update modal"),
                    _ => {}
                },
                FlakeModalState::Ready { .. } => match key.code {
                    KeyCode::Char('y') => {
                        let taken = std::mem::replace(&mut self.app.stage, Stage::Building);
                        if let Stage::FlakeUpdateModal(FlakeModalState::Ready { result }) = taken {
                            self.start_build(Some(result));
                        }
                    }
                    KeyCode::Char('n') => {
                        self.app.log_status("Skipped flake update");
                        self.start_build(None);
                    }
                    KeyCode::Char('q') => self.request_abort("Aborted at flake update modal"),
                    _ => {}
                },
                FlakeModalState::Failed { .. } => match key.code {
                    KeyCode::Enter | KeyCode::Char('n') => {
                        self.app.log_status("Continuing without flake update");
                        self.start_build(None);
                    }
                    KeyCode::Char('q') => self.request_abort("Aborted after flake update error"),
                    _ => {}
                },
            },
            Stage::ConfirmAbortRollback { op_id, .. } => {
                let op_id = op_id.clone();
                match key.code {
                    KeyCode::Char('r') => match repo::restore_op(&self.cfg.root, &op_id) {
                        Ok(()) => {
                            self.app
                                .exit_summary
                                .push("Reverted Bumps commit via jj op restore".to_string());
                            self.app.pre_bump_op_id = None;
                            self.app.should_quit = true;
                        }
                        Err(err) => {
                            self.app.fatal_error =
                                Some(format!("jj op restore {op_id} failed: {err:#}"));
                            self.app.should_quit = true;
                        }
                    },
                    KeyCode::Char('k') => {
                        self.app
                            .exit_summary
                            .push("Kept Bumps commit in jj history".to_string());
                        self.app.should_quit = true;
                    }
                    _ => {}
                }
            }
            Stage::PushBeforeLocalReboot {
                machine_idx,
                pushing,
                ..
            } => {
                if *pushing {
                    return;
                }
                let idx = *machine_idx;
                match key.code {
                    KeyCode::Char('p') => {
                        *pushing = true;
                        self.spawn_push(Some(idx));
                    }
                    KeyCode::Char('r') => {
                        let machine = self.app.machine_plans[idx].machine.clone();
                        self.app.machine_plans[idx].outcome =
                            MachineOutcome::Activated(DeployAction::BootReboot);
                        self.app.pending_local_reboot = Some(machine);
                        self.finish_cleanly();
                    }
                    KeyCode::Char('c') | KeyCode::Esc => {
                        let name = self.app.machine_plans[idx].machine.name.clone();
                        self.app.machine_plans[idx].outcome =
                            MachineOutcome::RebootCancelledAfterBoot;
                        self.app
                            .exit_summary
                            .push(format!("{name}: boot entry set; reboot cancelled"));
                        self.finish_cleanly();
                    }
                    _ => {}
                }
            }
            Stage::PushAfterDeploy { pushing, .. } => {
                if *pushing {
                    return;
                }
                match key.code {
                    KeyCode::Char('p') => {
                        *pushing = true;
                        self.spawn_push(None);
                    }
                    KeyCode::Enter | KeyCode::Char('q') | KeyCode::Esc => {
                        self.finish_cleanly();
                    }
                    _ => {}
                }
            }
            _ => self.handle_main_view_key(key),
        }
    }

    fn handle_main_view_key(&mut self, key: KeyEvent) {
        // Common navigation & diff controls active across main two-pane stages.
        match key.code {
            KeyCode::Tab => {
                self.app.active_pane = self.app.active_pane.toggle();
                return;
            }
            KeyCode::Left => {
                self.app.active_pane = Pane::Left;
                return;
            }
            KeyCode::Right => {
                self.app.active_pane = Pane::Right;
                return;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.app.scroll_active(1);
                return;
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.app.scroll_active(-1);
                return;
            }
            KeyCode::PageDown => {
                self.app.scroll_active(15);
                return;
            }
            KeyCode::PageUp => {
                self.app.scroll_active(-15);
                return;
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.app.scroll_active(15);
                return;
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.app.scroll_active(-15);
                return;
            }
            KeyCode::Char('n') => {
                self.app.active_pane = Pane::Left;
                self.app.jump_next_header();
                return;
            }
            KeyCode::Char('N') => {
                self.app.active_pane = Pane::Left;
                self.app.jump_prev_header();
                return;
            }
            KeyCode::Char('f') => {
                self.app.toggle_filtered_hunks();
                return;
            }
            KeyCode::Char('e') => {
                self.app.toggle_expand_large();
                return;
            }
            KeyCode::Char('q') => {
                if matches!(self.app.stage, Stage::Activating { .. }) {
                    self.app.log_status(
                        "Deployment is running; wait for completion (or press Ctrl-C to force exit)",
                    );
                } else {
                    self.request_abort("Aborted by user");
                }
                return;
            }
            _ => {}
        }

        // Stage-specific action keys.
        match self.app.stage {
            Stage::ReviewGate => {
                if key.code == KeyCode::Char('a') {
                    self.app.diffs_approved = true;
                    self.app.active_pane = Pane::Right;
                    self.app
                        .log_status("Flake diffs approved; proceeding to deployment");
                    self.advance_machine_flow(0);
                }
            }
            Stage::MachinePrompt { machine_idx } => match key.code {
                KeyCode::Char('s') => self.spawn_activation(machine_idx, DeployAction::Switch),
                KeyCode::Char('b') => self.spawn_activation(machine_idx, DeployAction::Boot),
                KeyCode::Char('r') => self.spawn_activation(machine_idx, DeployAction::BootReboot),
                _ => {}
            },
            _ => {}
        }
    }
}

impl Drop for TuiRuntime {
    fn drop(&mut self) {
        if let Some(pending) = self.pending_flake.take() {
            pending.cancel();
        }
        if let Some(handle) = self.build_handle.take() {
            handle.cancel();
        }
    }
}

/// Run the full interactive `ratatui` TUI deploy session.
///
/// Restores the terminal on every exit path (normal exit, error, or panic),
/// kills any background `nix` child processes before returning, and prints a
/// concise exit summary to stderr after leaving the alternate screen.
///
/// # Errors
///
/// Returns an error if initialization fails or if the deploy session fails.
pub fn run(cfg: TuiConfig) -> Result<()> {
    let mut guard = TerminalGuard::enter()?;
    let mut runtime = TuiRuntime::new(cfg);
    runtime.spawn_repo_inspect(false);

    while !runtime.app.should_quit {
        runtime.poll_background();
        if runtime.app.should_quit {
            break;
        }

        guard
            .terminal
            .draw(|frame| render::draw(frame, &runtime.app, &runtime.sysmon, &runtime.build_model))
            .context("drawing TUI frame")?;

        if event::poll(POLL_INTERVAL).context("polling terminal events")? {
            if let Event::Key(key) = event::read().context("reading terminal event")? {
                runtime.handle_key(key);
            }
        }
        runtime.app.tick = runtime.app.tick.wrapping_add(1);
    }

    // Extract exit summary and optional local reboot before dropping runtime & terminal.
    let summary = std::mem::take(&mut runtime.app.exit_summary);
    let fatal_error = runtime.app.fatal_error.take();
    let error_details = std::mem::take(&mut runtime.app.error_details);
    let pending_reboot = runtime.app.pending_local_reboot.take();

    // Drop runtime first to kill/reap any child processes, then restore terminal.
    drop(runtime);
    drop(guard);

    for line in &summary {
        eprintln!("{line}");
    }
    if !error_details.is_empty() {
        eprintln!("\n--- details ---");
        for line in &error_details {
            eprintln!("{line}");
        }
    }
    if let Some(err) = fatal_error {
        bail!("{err}");
    }

    if let Some(machine) = pending_reboot {
        eprintln!("rebooting {} now; goodbye", machine.name);
        machine
            .target
            .run_streamed(&["systemctl", "reboot"], true)?;
    }

    Ok(())
}

fn run_lineage_check(
    root: &Path,
    machines: &[Machine],
    strict: bool,
    allow_rollback: bool,
) -> Result<LineageSummary> {
    let deploying = lineage::deploying_rev(root)?;
    let mut warnings = Vec::new();
    let mut refusals = Vec::new();
    let mut running_revs = BTreeMap::new();

    for machine in machines {
        let running = lineage::running(machine, root, &deploying)
            .with_context(|| format!("checking what {} runs", machine.name))?;
        match &running {
            lineage::Running::Ancestor(rev) | lineage::Running::Rewritten(rev) => {
                running_revs.insert(machine.name.clone(), rev.commit.clone());
            }
            _ => {}
        }
        match lineage::judge(&machine.name, &running, strict) {
            lineage::Verdict::Ok => {}
            lineage::Verdict::Warn(msg) => warnings.push(msg),
            lineage::Verdict::Refuse(msg) if allow_rollback => {
                warnings.push(format!("{msg} (deploying anyway: --allow-rollback)"));
            }
            lineage::Verdict::Refuse(msg) => refusals.push(msg),
        }
    }

    Ok(LineageSummary {
        warnings,
        refusals,
        running_revs,
    })
}

fn compute_machine_plans(
    machines: &[Machine],
    toplevels: &BTreeMap<String, String>,
) -> Result<Vec<MachinePlan>> {
    let mut plans = Vec::with_capacity(machines.len());
    for machine in machines {
        let new_system = toplevels
            .get(&machine.name)
            .with_context(|| format!("missing built out path for {}", machine.name))?
            .clone();
        let old_system = diff::current_system(&machine.target)
            .with_context(|| format!("reading current system on {}", machine.name))?;
        if old_system == new_system {
            plans.push(MachinePlan {
                machine: machine.clone(),
                new_system,
                up_to_date: true,
                verdict: diff::Verdict::default(),
                nvd_lines: Vec::new(),
                outcome: MachineOutcome::UpToDate,
                activation_logs: Vec::new(),
            });
            continue;
        }
        let verdict = diff::assess(&machine.target, &old_system, &new_system)
            .with_context(|| format!("assessing closure diff for {}", machine.name))?;
        let nvd_lines = diff::capture_nvd_diff(&old_system, &new_system);
        plans.push(MachinePlan {
            machine: machine.clone(),
            new_system,
            up_to_date: false,
            verdict,
            nvd_lines,
            outcome: MachineOutcome::Pending,
            activation_logs: Vec::new(),
        });
    }
    Ok(plans)
}

fn run_activation<F>(
    machine: &Machine,
    new_system: &str,
    action: DeployAction,
    reboot_timeout: Duration,
    mut on_line: F,
) -> Result<()>
where
    F: FnMut(String),
{
    activate::copy_closure_streaming(&machine.target, new_system, &mut on_line)?;
    activate::activate_streaming(&machine.target, new_system, action.mode(), &mut on_line)?;

    match action {
        DeployAction::Switch => Ok(()),
        DeployAction::Boot => {
            on_line(format!(
                "{}: boot entry set; reboot it when convenient",
                machine.name
            ));
            Ok(())
        }
        DeployAction::BootReboot => {
            if machine.target.is_local() {
                // Local reboot happens right after the optional push-before-reboot prompt
                // and after leaving the alternate screen.
                on_line(format!("{}: boot entry set; ready to reboot", machine.name));
                Ok(())
            } else {
                activate::reboot_and_wait_streaming(
                    &machine.target,
                    new_system,
                    reboot_timeout,
                    on_line,
                )
            }
        }
    }
}

fn prev_char_boundary(s: &str, idx: usize) -> usize {
    if idx == 0 {
        return 0;
    }
    let mut i = idx.min(s.len()) - 1;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn next_char_boundary(s: &str, idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    let mut i = idx + 1;
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}
