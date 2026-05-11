//! Stateless build-candidate decision.
//!
//! Spec §"Scheduler": one function. Drop wrong-system targets, drop stale-
//! PONG or memory-low targets, compute `completion_ms = queue_ms +
//! package_ms × speed_multiplier`, pick the smallest, decline if the winner
//! is the controller's own host.
//!
//! Admissions are the **only** load signal — `nix_slots_active` is reported
//! by agents for divergence observability but does not enter this function.

use crate::persistence::admissions::AdmissionRow;
use crate::protocol::ops::{AcceptTarget, DecideCandidate, TelemetryBody};
use crate::util::pname_from_drv;

/// Static description of one routable build site.
#[derive(Clone, Debug)]
pub struct Target {
    pub name: String,
    /// TCP endpoint the controller dials to maintain its polling
    /// connection. For an agent running on the controller host this is
    /// typically `127.0.0.1:8765`.
    pub tcp_addr: std::net::SocketAddr,
    pub store_uri: String,
    pub builder_line: String,
    pub capacity: u32,
    pub speed_multiplier: f64,
    /// `true` if this target is the controller's own agent (the host that
    /// invokes `nixos-rebuild`). The scheduler never delegates to it; if the
    /// minimum-completion winner is this target, the decision is `Decline`
    /// so Nix builds locally.
    pub is_controller_host: bool,
}

/// Live state the controller maintains per target.
#[derive(Clone, Debug)]
pub struct TargetState {
    pub target: Target,
    /// Wall-clock time of the most recent `PONG` from this target.
    pub last_pong_ms: Option<u64>,
    pub last_telemetry: Option<TelemetryBody>,
}

#[derive(Clone, Debug)]
pub struct SchedulerPolicy {
    pub min_remote_mem_available_kb: u64,
    pub unknown_p95_ms: u64,
}

pub struct SchedulerInputs<'a> {
    pub system: &'a str,
    pub candidate: &'a DecideCandidate,
    pub now_ms: u64,
    pub poll_interval_ms: u64,
    pub policy: &'a SchedulerPolicy,
    pub admissions: &'a [AdmissionRow],
    pub targets: &'a [TargetState],
    /// Pre-fetched `p95_ms` for `pname_from_drv(candidate.drv_path)`, or
    /// `None` when the controller has no observations for this pname.
    pub p95_for_pname: Option<u64>,
}

/// What the scheduler decided.
///
/// - `Decline` — no eligible target (wrong system, all stale/low-mem, empty
///   list). The controller returns `Decision::Decline` to the hook and
///   records no admission.
/// - `RouteLocal` — the minimum-completion winner is the controller's own
///   host. Nix builds it locally. The controller still admits a row for
///   that target so the local in-flight queue is reflected in `queue_ms`;
///   the matching `EVENT_BUILD_FINISH` (from the local agent) retires it
///   on the same path as remote builds.
/// - `Accept` — delegate to a remote target.
#[derive(Clone, Debug, PartialEq)]
pub enum SchedulerDecision {
    Decline,
    RouteLocal {
        target_name: String,
        predicted_ms: u64,
    },
    Accept {
        target: AcceptTarget,
        predicted_ms: u64,
    },
}

pub fn decide(inputs: &SchedulerInputs) -> SchedulerDecision {
    let _pname = pname_from_drv(&inputs.candidate.drv_path);

    if inputs.candidate.system != inputs.system {
        return SchedulerDecision::Decline;
    }

    let stale_after_ms = inputs.poll_interval_ms.saturating_mul(3);
    let live: Vec<&TargetState> = inputs
        .targets
        .iter()
        .filter(|state| is_live(state, inputs.now_ms, stale_after_ms, inputs.policy))
        .collect();

    let package_ms_base = inputs.p95_for_pname.unwrap_or(inputs.policy.unknown_p95_ms);

    let mut best: Option<(&TargetState, u64, u64)> = None;
    for state in live {
        let target = &state.target;
        let package_ms = scaled_package_ms(package_ms_base, target.speed_multiplier);
        let queue_load_ms: u64 = inputs
            .admissions
            .iter()
            .filter(|a| a.target_name == target.name)
            .map(|a| a.predicted_ms)
            .sum();
        let queue_ms = if target.capacity == 0 {
            u64::MAX
        } else {
            queue_load_ms / target.capacity as u64
        };
        let completion_ms = queue_ms.saturating_add(package_ms);
        let replace = match best {
            None => true,
            Some((_, best_completion, _)) => completion_ms < best_completion,
        };
        if replace {
            best = Some((state, completion_ms, package_ms));
        }
    }

    let Some((winner, _completion, package_ms)) = best else {
        return SchedulerDecision::Decline;
    };

    if winner.target.is_controller_host {
        return SchedulerDecision::RouteLocal {
            target_name: winner.target.name.clone(),
            predicted_ms: package_ms.max(1),
        };
    }

    SchedulerDecision::Accept {
        target: AcceptTarget {
            name: winner.target.name.clone(),
            store_uri: winner.target.store_uri.clone(),
            builder_line: winner.target.builder_line.clone(),
        },
        predicted_ms: package_ms.max(1),
    }
}

fn is_live(
    state: &TargetState,
    now_ms: u64,
    stale_after_ms: u64,
    policy: &SchedulerPolicy,
) -> bool {
    let Some(last_pong_ms) = state.last_pong_ms else {
        return false;
    };
    if now_ms.saturating_sub(last_pong_ms) > stale_after_ms {
        return false;
    }
    let Some(telemetry) = state.last_telemetry.as_ref() else {
        return false;
    };
    telemetry.mem_available_kb >= policy.min_remote_mem_available_kb
}

fn scaled_package_ms(base_ms: u64, speed_multiplier: f64) -> u64 {
    let scaled = (base_ms as f64) * speed_multiplier.max(0.0);
    if !scaled.is_finite() || scaled < 0.0 {
        return base_ms;
    }
    scaled.round() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ops::TelemetryBody;

    const SYSTEM: &str = "x86_64-linux";

    fn policy() -> SchedulerPolicy {
        SchedulerPolicy {
            min_remote_mem_available_kb: 1_000_000,
            unknown_p95_ms: 60_000,
        }
    }

    fn ok_telemetry(slots: u32) -> TelemetryBody {
        TelemetryBody {
            mem_available_kb: 4_000_000,
            psi_memory_some_avg10: Some(0.0),
            nix_slots_active: slots,
            sampled_at_ms: 1_000,
        }
    }

    fn target(name: &str, capacity: u32, is_controller_host: bool) -> Target {
        Target {
            name: name.to_string(),
            tcp_addr: "127.0.0.1:0".parse().unwrap(),
            store_uri: format!("ssh-ng://svein@{name}.local"),
            builder_line: format!("ssh-ng://svein@{name}.local x86_64-linux . 1 1 - - -"),
            capacity,
            speed_multiplier: 1.0,
            is_controller_host,
        }
    }

    fn fresh_state(name: &str, capacity: u32, is_local: bool) -> TargetState {
        TargetState {
            target: target(name, capacity, is_local),
            last_pong_ms: Some(1_000),
            last_telemetry: Some(ok_telemetry(0)),
        }
    }

    fn candidate(drv: &str) -> DecideCandidate {
        DecideCandidate {
            drv_path: drv.to_string(),
            system: SYSTEM.to_string(),
            required_features: vec![],
            hook_pid: 12345,
        }
    }

    fn run(
        targets: &[TargetState],
        admissions: &[AdmissionRow],
        p95: Option<u64>,
    ) -> SchedulerDecision {
        let cand = candidate("/nix/store/abc-foo-1.2.3.drv");
        let pol = policy();
        decide(&SchedulerInputs {
            system: SYSTEM,
            candidate: &cand,
            now_ms: 1_000,
            poll_interval_ms: 1_000,
            policy: &pol,
            admissions,
            targets,
            p95_for_pname: p95,
        })
    }

    #[test]
    fn wrong_system_declines() {
        let cand = DecideCandidate {
            drv_path: "/nix/store/abc-foo.drv".to_string(),
            system: "aarch64-darwin".to_string(),
            required_features: vec![],
            hook_pid: 0,
        };
        let pol = policy();
        let ts = [fresh_state("tsugumi", 16, false)];
        let decision = decide(&SchedulerInputs {
            system: SYSTEM,
            candidate: &cand,
            now_ms: 1_000,
            poll_interval_ms: 1_000,
            policy: &pol,
            admissions: &[],
            targets: &ts,
            p95_for_pname: None,
        });
        assert_eq!(decision, SchedulerDecision::Decline);
    }

    #[test]
    fn single_controller_host_target_routes_local() {
        // Spec: "Single-target case (only controller host's agent) → always Decline."
        // The scheduler reports this as RouteLocal so the controller can
        // admit the local in-flight build; the wire-level Decision sent back
        // to the hook is still Decline.
        let ts = [fresh_state("saya", 8, true)];
        let decision = run(&ts, &[], Some(5_000));
        match decision {
            SchedulerDecision::RouteLocal { target_name, .. } => {
                assert_eq!(target_name, "saya");
            }
            other => panic!("expected RouteLocal saya, got {other:?}"),
        }
    }

    #[test]
    fn unknown_history_uses_unknown_p95_and_picks_best() {
        // Two targets, equal capacity, no admissions, no history. With
        // tied completion_ms the first target wins by iteration order.
        let ts = [
            fresh_state("tsugumi", 8, false),
            fresh_state("kaho", 8, false),
        ];
        let decision = run(&ts, &[], None);
        match decision {
            SchedulerDecision::Accept {
                target,
                predicted_ms,
            } => {
                assert_eq!(target.name, "tsugumi");
                assert_eq!(predicted_ms, 60_000);
            }
            other => panic!("expected accept tsugumi, got {other:?}"),
        }
    }

    #[test]
    fn memory_low_target_excluded_routing_falls_back() {
        let mut a = fresh_state("tsugumi", 8, false);
        a.last_telemetry = Some(TelemetryBody {
            mem_available_kb: 100_000,
            ..ok_telemetry(0)
        });
        let b = fresh_state("kaho", 8, false);
        let decision = run(&[a, b], &[], Some(10_000));
        match decision {
            SchedulerDecision::Accept { target, .. } => assert_eq!(target.name, "kaho"),
            other => panic!("expected accept kaho, got {other:?}"),
        }
    }

    #[test]
    fn all_targets_memory_low_declines() {
        let mut a = fresh_state("tsugumi", 8, false);
        a.last_telemetry = Some(TelemetryBody {
            mem_available_kb: 100_000,
            ..ok_telemetry(0)
        });
        let mut b = fresh_state("kaho", 8, false);
        b.last_telemetry = Some(TelemetryBody {
            mem_available_kb: 100_000,
            ..ok_telemetry(0)
        });
        let decision = run(&[a, b], &[], Some(10_000));
        assert_eq!(decision, SchedulerDecision::Decline);
    }

    #[test]
    fn stale_pong_target_excluded() {
        // poll_interval 1s × 3 = 3s. Pong at t=0 means age 1000 > 3000? No.
        // Use age 5_000 (now=1000, last_pong=-4000 saturating). Set
        // last_pong to None to force stale.
        let mut a = fresh_state("tsugumi", 8, false);
        a.last_pong_ms = None;
        let b = fresh_state("kaho", 8, false);
        let decision = run(&[a, b], &[], Some(10_000));
        match decision {
            SchedulerDecision::Accept { target, .. } => assert_eq!(target.name, "kaho"),
            other => panic!("expected accept kaho, got {other:?}"),
        }
    }

    #[test]
    fn stale_pong_by_age_excludes() {
        // poll_interval 1s × 3 = 3s stale threshold. tsugumi's last_pong was
        // ~100ms after epoch — at now=1_000_000 that's far older than 3s.
        // kaho ponged at 999_500ms (500ms ago) — still live.
        let mut a = fresh_state("tsugumi", 8, false);
        a.last_pong_ms = Some(100);
        let mut b = fresh_state("kaho", 8, false);
        b.last_pong_ms = Some(999_500);
        let cand = candidate("/nix/store/abc-foo-1.drv");
        let pol = policy();
        let decision = decide(&SchedulerInputs {
            system: SYSTEM,
            candidate: &cand,
            now_ms: 1_000_000,
            poll_interval_ms: 1_000,
            policy: &pol,
            admissions: &[],
            targets: &[a, b],
            p95_for_pname: Some(10_000),
        });
        match decision {
            SchedulerDecision::Accept { target, .. } => assert_eq!(target.name, "kaho"),
            other => panic!("expected accept kaho, got {other:?}"),
        }
    }

    #[test]
    fn speed_multiplier_halves_package_estimate() {
        let mut fast = fresh_state("tsugumi", 8, false);
        fast.target.speed_multiplier = 0.5;
        let slow = fresh_state("kaho", 8, false);
        let decision = run(&[fast, slow], &[], Some(10_000));
        match decision {
            SchedulerDecision::Accept {
                target,
                predicted_ms,
            } => {
                assert_eq!(target.name, "tsugumi");
                assert_eq!(predicted_ms, 5_000, "5_000 = 10_000 × 0.5");
            }
            other => panic!("expected accept tsugumi, got {other:?}"),
        }
    }

    #[test]
    fn admissions_accumulate_queue_ms() {
        // tsugumi has 2 admissions × 30s each / capacity 8 → queue_ms = 7_500.
        // kaho is idle with capacity 8 → queue_ms = 0.
        // package_ms = 5_000 for both.
        // tsugumi completion = 7_500 + 5_000 = 12_500.
        // kaho completion = 0 + 5_000 = 5_000 → kaho wins.
        let ts = [
            fresh_state("tsugumi", 8, false),
            fresh_state("kaho", 8, false),
        ];
        let admissions = vec![
            AdmissionRow {
                drv_path: "x".into(),
                target_name: "tsugumi".into(),
                admitted_at_ms: 0,
                predicted_ms: 30_000,
            },
            AdmissionRow {
                drv_path: "y".into(),
                target_name: "tsugumi".into(),
                admitted_at_ms: 0,
                predicted_ms: 30_000,
            },
        ];
        let decision = run(&ts, &admissions, Some(5_000));
        match decision {
            SchedulerDecision::Accept { target, .. } => assert_eq!(target.name, "kaho"),
            other => panic!("expected kaho, got {other:?}"),
        }
    }

    #[test]
    fn admissions_for_different_target_do_not_count() {
        // 10 admissions on kaho should not push tsugumi's queue at all.
        let ts = [
            fresh_state("tsugumi", 8, false),
            fresh_state("kaho", 8, false),
        ];
        let mut admissions = Vec::new();
        for i in 0..10 {
            admissions.push(AdmissionRow {
                drv_path: format!("/k-{i}.drv"),
                target_name: "kaho".to_string(),
                admitted_at_ms: 0,
                predicted_ms: 60_000,
            });
        }
        let decision = run(&ts, &admissions, Some(5_000));
        match decision {
            SchedulerDecision::Accept { target, .. } => assert_eq!(target.name, "tsugumi"),
            other => panic!("expected tsugumi, got {other:?}"),
        }
    }

    #[test]
    fn nix_slots_active_is_not_a_load_signal() {
        // Two targets identical except tsugumi reports 16 active slots.
        // With no admissions, the decision must NOT prefer kaho. Spec
        // explicitly rules out double-counting slots and admissions.
        let mut a = fresh_state("tsugumi", 8, false);
        a.last_telemetry = Some(ok_telemetry(16));
        let b = fresh_state("kaho", 8, false);
        let decision = run(&[a, b], &[], Some(5_000));
        match decision {
            SchedulerDecision::Accept { target, .. } => assert_eq!(target.name, "tsugumi"),
            other => panic!("expected tsugumi (first iter), got {other:?}"),
        }
    }

    #[test]
    fn empty_target_list_declines() {
        let decision = run(&[], &[], Some(5_000));
        assert_eq!(decision, SchedulerDecision::Decline);
    }

    #[test]
    fn winner_is_controller_host_routes_local() {
        // Two targets; controller host has lower queue, so it wins on
        // completion_ms. The scheduler returns RouteLocal so the controller
        // can record an admission for the local in-flight build; the wire-
        // level Decision is still Decline.
        let mut local = fresh_state("saya", 16, true);
        local.target.capacity = 32;
        let remote = fresh_state("tsugumi", 8, false);
        let admissions = vec![AdmissionRow {
            drv_path: "/q.drv".into(),
            target_name: "tsugumi".into(),
            admitted_at_ms: 0,
            predicted_ms: 30_000,
        }];
        let decision = run(&[local, remote], &admissions, Some(5_000));
        match decision {
            SchedulerDecision::RouteLocal { target_name, .. } => {
                assert_eq!(target_name, "saya");
            }
            other => panic!("expected RouteLocal saya, got {other:?}"),
        }
    }

    #[test]
    fn predicted_ms_is_at_least_one() {
        // package_ms_base = 0 (history says zero-duration), speed = 1.
        let ts = [fresh_state("tsugumi", 8, false)];
        let decision = run(&ts, &[], Some(0));
        match decision {
            SchedulerDecision::Accept { predicted_ms, .. } => assert_eq!(predicted_ms, 1),
            other => panic!("expected accept, got {other:?}"),
        }
    }

    #[test]
    fn zero_capacity_target_never_wins_over_normal() {
        let mut broken = fresh_state("zero", 0, false);
        broken.target.capacity = 0;
        let good = fresh_state("tsugumi", 8, false);
        let decision = run(&[broken, good], &[], Some(5_000));
        match decision {
            SchedulerDecision::Accept { target, .. } => assert_eq!(target.name, "tsugumi"),
            other => panic!("expected tsugumi, got {other:?}"),
        }
    }
}
