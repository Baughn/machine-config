use crate::api::types::{BuildCandidate, Decision, DecisionMetrics, PackageStats};
use crate::persistence::queries::sample_prediction_ms;
use crate::scheduler::policy::{BuildTarget, SchedulerConfig, SchedulerPolicy};
use crate::scheduler::state::{DecisionOutcome, Eligibility, HostState, Prediction};

pub fn evaluate_candidate_compatibility(candidate: &BuildCandidate) -> Eligibility {
    if candidate.needed_system == "x86_64-linux" || candidate.needed_system == "builtin" {
        Eligibility::Accepted
    } else {
        Eligibility::Declined {
            reason: "unsupported system",
        }
    }
}

pub fn evaluate_remote_health(
    remote: &HostState,
    now: u128,
    policy: &SchedulerPolicy,
) -> Eligibility {
    if now.saturating_sub(remote.telemetry.timestamp_ms) > policy.stale_telemetry_ms {
        return Eligibility::Declined {
            reason: "remote telemetry is stale",
        };
    }
    if remote.telemetry.cpu_busy_ratio.unwrap_or(1.0) > policy.max_remote_cpu_busy_ratio {
        return Eligibility::Declined {
            reason: "remote cpu is busy",
        };
    }
    if remote.telemetry.psi_memory_some_avg10.unwrap_or(0.0)
        > policy.max_remote_memory_pressure_avg10
    {
        return Eligibility::Declined {
            reason: "remote memory pressure is high",
        };
    }
    if remote.telemetry.mem_available_kb.unwrap_or(0) < policy.min_remote_mem_available_kb {
        return Eligibility::Declined {
            reason: "remote memory is low",
        };
    }
    Eligibility::Accepted
}

pub fn evaluate_predictions(
    local: &HostState,
    remote: &HostState,
    scheduler: &SchedulerConfig,
) -> (Prediction, Prediction, bool) {
    let (local_package_ms, remote_package_ms) = paired_predictions_with_policy(
        local.stats.as_ref(),
        remote.stats.as_ref(),
        &scheduler.policy,
    );
    let local_samples = local.stats.as_ref().map(|stats| stats.count).unwrap_or(0);
    let remote_samples = remote.stats.as_ref().map(|stats| stats.count).unwrap_or(0);
    let local_queue_ms = local_queue_ms(local, &scheduler.policy);
    let remote_queue_ms = remote_queue_ms(remote, &scheduler.remote_target, &scheduler.policy);
    let local_prediction = Prediction {
        samples: local_samples,
        package_ms: local_package_ms,
        queue_ms: local_queue_ms,
        completion_ms: local_queue_ms + local_package_ms,
    };
    let remote_prediction = Prediction {
        samples: remote_samples,
        package_ms: remote_package_ms,
        queue_ms: remote_queue_ms,
        completion_ms: remote_queue_ms + remote_package_ms,
    };
    let unknown = local_samples == 0 && remote_samples == 0;
    (local_prediction, remote_prediction, unknown)
}

pub fn evaluate_remote_admission_limits(
    remote: &HostState,
    unknown: bool,
    now: u128,
    policy: &SchedulerPolicy,
) -> Eligibility {
    if remote.admissions.len() >= policy.max_remote_admitted {
        return Eligibility::Declined {
            reason: "remote admission limit reached",
        };
    }
    if unknown
        && remote
            .admissions
            .iter()
            .filter(|admission| admission.unknown)
            .count()
            >= policy.max_unknown_remote
    {
        return Eligibility::Declined {
            reason: "unknown remote admission limit reached",
        };
    }
    if let Some(last) = remote
        .admissions
        .iter()
        .map(|admission| admission.admitted_at_ms)
        .max()
    {
        if now.saturating_sub(last as u128) < policy.min_remote_admission_interval_ms {
            return Eligibility::Declined {
                reason: "remote admission interval not elapsed",
            };
        }
    }
    Eligibility::Accepted
}

fn local_queue_ms(local: &HostState, policy: &SchedulerPolicy) -> u64 {
    let local_slot_queue_ms = (local.telemetry.nix_slots_local as u64 * policy.unknown_p95_ms)
        / policy.local_capacity as u64;
    local_slot_queue_ms.max(local.active_queue_ms)
}

fn remote_queue_ms(remote: &HostState, target: &BuildTarget, policy: &SchedulerPolicy) -> u64 {
    let remote_existing_ms =
        (remote.telemetry.nix_slots_total as u64 * policy.unknown_p95_ms) / target.capacity as u64;
    let admitted_ms: u64 = remote
        .admissions
        .iter()
        .map(|admission| admission.predicted_ms)
        .sum();
    remote_existing_ms + admitted_ms / target.capacity as u64
}

pub fn decision_metrics(
    local: &HostState,
    remote: &HostState,
    local_prediction: &Prediction,
    remote_prediction: &Prediction,
) -> DecisionMetrics {
    DecisionMetrics {
        local_samples: local_prediction.samples,
        remote_samples: remote_prediction.samples,
        local_prediction_ms: local_prediction.package_ms,
        remote_prediction_ms: remote_prediction.package_ms,
        local_queue_ms: local_prediction.queue_ms,
        remote_queue_ms: remote_prediction.queue_ms,
        local_completion_ms: local_prediction.completion_ms,
        remote_completion_ms: remote_prediction.completion_ms,
        local_slots: local.telemetry.nix_slots_local,
        remote_slots: remote.telemetry.nix_slots_total,
        local_active_count: local.active_count,
        admitted_count: remote.admissions.len(),
    }
}

pub fn compare_predictions(
    candidate: &BuildCandidate,
    target: &BuildTarget,
    metrics: DecisionMetrics,
    policy: &SchedulerPolicy,
) -> DecisionOutcome {
    let explore = should_explore_empty_host(candidate, &metrics, policy);

    if metrics.remote_completion_ms >= metrics.local_completion_ms {
        if explore && remote_host_is_empty(&metrics) {
            return DecisionOutcome {
                record_remote_admission: true,
                decision: Decision {
                    decision: "accept".to_string(),
                    reason: "exploration: empty remote host selected".to_string(),
                    store_uri: Some(target.store_uri.clone()),
                    metrics: Some(metrics),
                },
            };
        }
        return DecisionOutcome {
            record_remote_admission: false,
            decision: Decision {
                decision: "decline".to_string(),
                reason: "local queue is predicted to drain sooner".to_string(),
                store_uri: None,
                metrics: Some(metrics),
            },
        };
    }

    if explore && local_host_is_empty(&metrics) {
        return DecisionOutcome {
            record_remote_admission: false,
            decision: Decision {
                decision: "decline".to_string(),
                reason: "exploration: empty local host selected".to_string(),
                store_uri: None,
                metrics: Some(metrics),
            },
        };
    }

    DecisionOutcome {
        record_remote_admission: true,
        decision: Decision {
            decision: "accept".to_string(),
            reason: format!(
                "remote predicted {}ms vs local {}ms",
                metrics.remote_completion_ms, metrics.local_completion_ms
            ),
            store_uri: Some(target.store_uri.clone()),
            metrics: Some(metrics),
        },
    }
}

fn should_explore_empty_host(
    candidate: &BuildCandidate,
    metrics: &DecisionMetrics,
    policy: &SchedulerPolicy,
) -> bool {
    if metrics.local_samples >= policy.exploration_min_samples
        && metrics.remote_samples >= policy.exploration_min_samples
    {
        return false;
    }
    stable_percent(&candidate.drv_path) < policy.exploration_percent
}

fn local_host_is_empty(metrics: &DecisionMetrics) -> bool {
    metrics.local_slots == 0 && metrics.local_active_count == 0
}

fn remote_host_is_empty(metrics: &DecisionMetrics) -> bool {
    metrics.remote_slots == 0 && metrics.admitted_count == 0
}

pub fn stable_percent(value: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in value.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash % 100
}

pub fn decline(reason: &str) -> Decision {
    Decision {
        decision: "decline".to_string(),
        reason: reason.to_string(),
        store_uri: None,
        metrics: None,
    }
}

pub fn paired_predictions_with_policy(
    local_stats: Option<&PackageStats>,
    remote_stats: Option<&PackageStats>,
    policy: &SchedulerPolicy,
) -> (u64, u64) {
    let local = sample_prediction_ms(local_stats);
    let remote = sample_prediction_ms(remote_stats);
    (
        local.or(remote).unwrap_or(policy.unknown_p95_ms),
        remote.or(local).unwrap_or(policy.unknown_p95_ms),
    )
}

#[cfg(test)]
fn paired_predictions(
    local_stats: Option<&PackageStats>,
    remote_stats: Option<&PackageStats>,
) -> (u64, u64) {
    paired_predictions_with_policy(local_stats, remote_stats, &SchedulerPolicy::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::policy::DEFAULT_UNKNOWN_P95_MS;
    use crate::test_support::{
        non_exploration_drv_path, test_candidate, test_metrics, test_target,
    };

    #[test]
    fn paired_predictions_borrow_missing_side_history() {
        let local = PackageStats {
            count: 1,
            p95_ms: 42_000,
        };
        let remote = PackageStats {
            count: 1,
            p95_ms: 24_000,
        };

        assert_eq!(paired_predictions(Some(&local), None), (42_000, 42_000));
        assert_eq!(paired_predictions(None, Some(&remote)), (24_000, 24_000));
        assert_eq!(
            paired_predictions(None, None),
            (DEFAULT_UNKNOWN_P95_MS, DEFAULT_UNKNOWN_P95_MS)
        );
    }

    #[test]
    fn comparison_declines_when_local_prediction_is_faster() {
        let candidate = test_candidate(&non_exploration_drv_path("local-faster"));
        let outcome = compare_predictions(
            &candidate,
            &test_target(),
            test_metrics(10_000, 20_000),
            &SchedulerPolicy::default(),
        );

        assert!(!outcome.record_remote_admission);
        assert_eq!(outcome.decision.decision, "decline");
        assert_eq!(
            outcome.decision.reason,
            "local queue is predicted to drain sooner"
        );
    }

    #[test]
    fn comparison_accepts_when_remote_prediction_is_faster() {
        let candidate = test_candidate(&non_exploration_drv_path("remote-faster"));
        let outcome = compare_predictions(
            &candidate,
            &test_target(),
            test_metrics(20_000, 10_000),
            &SchedulerPolicy::default(),
        );

        assert!(outcome.record_remote_admission);
        assert_eq!(outcome.decision.decision, "accept");
        assert_eq!(
            outcome.decision.reason,
            "remote predicted 10000ms vs local 20000ms"
        );
        assert_eq!(
            outcome.decision.store_uri.as_deref(),
            Some(crate::config::DEFAULT_REMOTE_STORE_URI)
        );
    }
}
