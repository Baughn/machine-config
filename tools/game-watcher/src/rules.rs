use anyhow::{anyhow, Result};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};
use tracing::{info, warn};

use crate::config::{Config, GuardAction};
use crate::firewall::{self, InsertedRule, ShellRunner};
use crate::gpu::{self, GpuMonitor};
use crate::service;

pub struct Engine {
    config: Config,
    /// Resolved guard state, parallel to `config.gpu_guards`.
    guards: Vec<GuardState>,
    active: HashMap<u32, GameSession>,
}

struct GameSession {
    name: String,
    firewall: Vec<InsertedRule>,
}

struct GuardState {
    guard_idx: usize,
    /// AppIds resolved from `requires_any_of` game names at init.
    requires_any_of_ids: Vec<u32>,
    /// AppIds resolved from `escalate.applies_if_any_of` game names at init.
    escalate_applies_ids: Vec<u32>,
    phase: Phase,
    below_since: Option<Instant>,
    history: VecDeque<Instant>,
    mode: Mode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Active,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal,
    Stopped,
}

impl Engine {
    pub fn new(config: Config) -> Result<Self> {
        let name_to_id: HashMap<&str, u32> = config
            .games
            .iter()
            .map(|g| (g.name.as_str(), g.app_id))
            .collect();

        let resolve = |names: &[String], context: &str| -> Result<Vec<u32>> {
            names
                .iter()
                .map(|n| {
                    name_to_id
                        .get(n.as_str())
                        .copied()
                        .ok_or_else(|| anyhow!("{context}: unknown game name '{n}'"))
                })
                .collect()
        };

        let mut guards = Vec::with_capacity(config.gpu_guards.len());
        for (i, g) in config.gpu_guards.iter().enumerate() {
            let requires = resolve(
                &g.requires_any_of,
                &format!("guard '{}' requires_any_of", g.name),
            )?;
            let escalate_ids = if let Some(esc) = &g.escalate {
                resolve(
                    &esc.applies_if_any_of,
                    &format!("guard '{}' escalate.applies_if_any_of", g.name),
                )?
            } else {
                Vec::new()
            };
            guards.push(GuardState {
                guard_idx: i,
                requires_any_of_ids: requires,
                escalate_applies_ids: escalate_ids,
                phase: Phase::Idle,
                below_since: None,
                history: VecDeque::new(),
                mode: Mode::Normal,
            });
        }

        Ok(Self {
            config,
            guards,
            active: HashMap::new(),
        })
    }

    pub fn on_game_start(&mut self, app_id: u32) {
        let Some(game) = self.config.games.iter().find(|g| g.app_id == app_id) else {
            info!(app_id, "unknown game started (no rules configured): app_id={app_id}");
            self.active.insert(
                app_id,
                GameSession {
                    name: format!("<unknown:{app_id}>"),
                    firewall: Vec::new(),
                },
            );
            return;
        };
        let name = game.name.clone();
        info!(app_id, game = %name, "game started: {name} ({app_id})");
        let firewall = match firewall::apply(app_id, &name, &game.firewall, &ShellRunner) {
            Ok(rules) => rules,
            Err(e) => {
                warn!(
                    app_id, game = %name, error = %e,
                    "failed to apply firewall rules for {name}: {e} (partial rules may exist)"
                );
                Vec::new()
            }
        };

        // Preemptive guard-action fire: free VRAM (etc.) as soon as a
        // guard-covered game starts, rather than waiting for a GPU spike.
        // Only Restart is safe to fire preemptively; skip if another applicable
        // game was already active, if escalation has stopped the service, or
        // if the guard's primary action isn't Restart.
        for gs in &mut self.guards {
            let guard = &self.config.gpu_guards[gs.guard_idx];
            if !gs.requires_any_of_ids.contains(&app_id) {
                continue;
            }
            if gs.mode == Mode::Stopped {
                continue;
            }
            if guard.action != GuardAction::Restart {
                continue;
            }
            let already_covered = gs
                .requires_any_of_ids
                .iter()
                .any(|id| *id != app_id && self.active.contains_key(id));
            if already_covered {
                continue;
            }
            info!(
                guard = %guard.name, service = %guard.service, game = %name,
                "preemptive restart of {} for guard {} on {} start",
                guard.service, guard.name, name
            );
            if let Err(e) = service::restart(&guard.service) {
                warn!(
                    guard = %guard.name, service = %guard.service, error = %e,
                    "preemptive restart of {} failed: {e}", guard.service
                );
            }
            gs.phase = Phase::Idle;
            gs.below_since = None;
        }

        self.active.insert(app_id, GameSession { name, firewall });
    }

    pub fn on_game_stop(&mut self, app_id: u32) {
        let Some(session) = self.active.remove(&app_id) else {
            return;
        };
        info!(app_id, game = %session.name, "game stopped: {} ({app_id})", session.name);

        if let Err(e) = firewall::revert(&session.name, app_id, &session.firewall, &ShellRunner) {
            warn!(
                app_id, game = %session.name, error = %e,
                "failed to fully revert firewall rules for {}: {e}", session.name
            );
        }

        // If any guard is in Stopped mode and this game's category was the
        // last applicable one running, restore the service.
        let active_ids: Vec<u32> = self.active.keys().copied().collect();
        for gs in &mut self.guards {
            let guard = &self.config.gpu_guards[gs.guard_idx];
            if gs.mode != Mode::Stopped {
                continue;
            }
            if !gs.escalate_applies_ids.contains(&app_id) {
                continue;
            }
            let any_still_active = active_ids
                .iter()
                .any(|id| gs.escalate_applies_ids.contains(id));
            if any_still_active {
                continue;
            }
            info!(
                guard = %guard.name,
                service = %guard.service,
                "game ended; starting {} for guard {}", guard.service, guard.name
            );
            if let Err(e) = service::start(&guard.service) {
                warn!(
                    guard = %guard.name, error = %e,
                    "failed to start {}: {e}", guard.service
                );
            }
            gs.mode = Mode::Normal;
            gs.history.clear();
            gs.phase = Phase::Idle;
            gs.below_since = None;
        }
    }

    /// Walk every GPU guard, sample util for its target service if any required
    /// game is running, advance the state machine, and fire actions.
    pub fn poll_gpu(&mut self, gpu: &mut GpuMonitor) -> Result<()> {
        let now = Instant::now();
        let active_ids: Vec<u32> = self.active.keys().copied().collect();

        for gs in &mut self.guards {
            let guard = &self.config.gpu_guards[gs.guard_idx];
            let guard_active = gs
                .requires_any_of_ids
                .iter()
                .any(|id| active_ids.contains(id));
            if !guard_active {
                gs.phase = Phase::Idle;
                gs.below_since = None;
                continue;
            }
            if gs.mode == Mode::Stopped {
                continue;
            }

            let Some(pid) = gpu::service_main_pid(&guard.service)? else {
                gs.phase = Phase::Idle;
                gs.below_since = None;
                continue;
            };

            let util_opt = gpu.sample_max_util(&[pid])?;
            let util = util_opt.unwrap_or(0);
            let threshold = guard.gpu_util_threshold_pct;
            let settle = Duration::from_secs(guard.settle_seconds);

            match gs.phase {
                Phase::Idle => {
                    if util > threshold {
                        info!(
                            guard = %guard.name, util,
                            "gpu spike on {} ({}%): entering Active", guard.name, util
                        );
                        gs.phase = Phase::Active;
                        gs.below_since = None;
                    }
                }
                Phase::Active => {
                    if util > threshold {
                        gs.below_since = None;
                    } else {
                        let anchor = *gs.below_since.get_or_insert(now);
                        if now.duration_since(anchor) >= settle {
                            info!(
                                guard = %guard.name,
                                settle_seconds = guard.settle_seconds,
                                "gpu settled on {} after {}s; firing guard action",
                                guard.name, guard.settle_seconds
                            );
                            fire_guard(gs, guard, &active_ids, now);
                            gs.phase = Phase::Idle;
                            gs.below_since = None;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Called on daemon shutdown: revert all firewall rules and restore any
    /// stopped services.
    pub fn shutdown(&mut self) {
        info!("shutting down engine; reverting all state");
        for (app_id, session) in self.active.drain() {
            if let Err(e) = firewall::revert(&session.name, app_id, &session.firewall, &ShellRunner) {
                warn!(
                    app_id, game = %session.name, error = %e,
                    "revert failed during shutdown for {}: {e}", session.name
                );
            }
        }
        for gs in &mut self.guards {
            if gs.mode == Mode::Stopped {
                let guard = &self.config.gpu_guards[gs.guard_idx];
                info!(
                    guard = %guard.name, service = %guard.service,
                    "restoring {} on shutdown (was stopped by guard {})", guard.service, guard.name
                );
                if let Err(e) = service::start(&guard.service) {
                    warn!(
                        guard = %guard.name, error = %e,
                        "failed to restart {} on shutdown: {e}", guard.service
                    );
                }
                gs.mode = Mode::Normal;
            }
        }
    }
}

fn fire_guard(
    gs: &mut GuardState,
    guard: &crate::config::GpuGuard,
    active_ids: &[u32],
    now: Instant,
) {
    let window = guard
        .escalate
        .as_ref()
        .map(|e| Duration::from_secs(e.within_seconds))
        .unwrap_or(Duration::from_secs(600));
    while let Some(front) = gs.history.front() {
        if now.duration_since(*front) > window {
            gs.history.pop_front();
        } else {
            break;
        }
    }
    gs.history.push_back(now);

    if let Some(esc) = &guard.escalate {
        let applies = gs
            .escalate_applies_ids
            .iter()
            .any(|id| active_ids.contains(id));
        if applies && gs.history.len() > esc.when_triggers_exceed {
            info!(
                guard = %guard.name,
                count = gs.history.len(),
                "escalation on {}: {} triggers within window; applying escalation action",
                guard.name, gs.history.len()
            );
            run_action(&guard.service, esc.action);
            if esc.action == GuardAction::Stop {
                gs.mode = Mode::Stopped;
            }
            return;
        }
    }

    run_action(&guard.service, guard.action);
    if guard.action == GuardAction::Stop {
        gs.mode = Mode::Stopped;
    }
}

fn run_action(service: &str, action: GuardAction) {
    let result = match action {
        GuardAction::Restart => service::restart(service),
        GuardAction::Stop => service::stop(service),
    };
    if let Err(e) = result {
        warn!(
            %service, ?action, error = %e,
            "guard action {:?} failed for {service}: {e}", action
        );
    }
}
