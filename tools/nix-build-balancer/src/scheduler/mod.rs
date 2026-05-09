pub mod eligibility;
pub mod host_state;
pub mod policy;
pub mod state;

use std::io;

use crate::api::types::{BuildCandidate, Decision};
use crate::config::Config;
use crate::persistence::cleanup::cleanup_stale_admissions_with_policy;
use crate::persistence::events::record_remote_admission_at;
use crate::persistence::open_history_db;
use crate::util::now_ms;

use eligibility::{
    compare_predictions, decision_metrics, decline, evaluate_candidate_compatibility,
    evaluate_predictions, evaluate_remote_admission_limits, evaluate_remote_health,
};
use host_state::{load_local_host_state, load_remote_host_state};
use policy::SchedulerConfig;
use state::Eligibility;

/// Decide whether one build candidate should run on the configured remote host.
pub fn decide_build_candidate(cfg: &Config, candidate: &BuildCandidate) -> io::Result<Decision> {
    let scheduler = SchedulerConfig::from_candidate(cfg, candidate);
    let conn = open_history_db(&cfg.data_dir)?;
    cleanup_stale_admissions_with_policy(&conn, &scheduler.policy)?;
    let decision_time_ms = now_ms();

    if let Eligibility::Declined { reason } = evaluate_candidate_compatibility(candidate) {
        return Ok(decline(reason));
    }

    let local = load_local_host_state(&conn, candidate, &scheduler, decision_time_ms)?;
    let remote = load_remote_host_state(&conn, cfg, candidate, &scheduler.remote_target)?;

    if let Eligibility::Declined { reason } =
        evaluate_remote_health(&remote, decision_time_ms, &scheduler.policy)
    {
        return Ok(decline(reason));
    }

    let (local_prediction, remote_prediction, unknown) =
        evaluate_predictions(&local, &remote, &scheduler);

    if let Eligibility::Declined { reason } =
        evaluate_remote_admission_limits(&remote, unknown, decision_time_ms, &scheduler.policy)
    {
        return Ok(decline(reason));
    }

    let metrics = decision_metrics(&local, &remote, &local_prediction, &remote_prediction);
    let outcome = compare_predictions(
        candidate,
        &scheduler.remote_target,
        metrics,
        &scheduler.policy,
    );

    if outcome.record_remote_admission {
        record_remote_admission_at(
            &conn,
            candidate,
            remote_prediction.package_ms,
            unknown,
            decision_time_ms,
        )?;
    }

    Ok(outcome.decision)
}

pub fn log_scheduler_decision(cfg: &Config, candidate: &BuildCandidate, decision: &Decision) {
    eprintln!("{}", scheduler_decision_log_line(cfg, candidate, decision));
}

fn scheduler_decision_log_line(
    cfg: &Config,
    candidate: &BuildCandidate,
    decision: &Decision,
) -> String {
    let required_features = candidate.required_features.join(",");
    let mut line = format!(
        "scheduler_decision host={} remote_host={} decision={} reason={} pname={} drv_path={} needed_system={} required_features={} store_uri={}",
        quoted_log_value(&cfg.host),
        quoted_log_value(&candidate.remote_host),
        quoted_log_value(&decision.decision),
        quoted_log_value(&decision.reason),
        quoted_log_value(&candidate.pname),
        quoted_log_value(&candidate.drv_path),
        quoted_log_value(&candidate.needed_system),
        quoted_log_value(&required_features),
        decision
            .store_uri
            .as_ref()
            .map(|value| quoted_log_value(value))
            .unwrap_or_else(|| "null".to_string())
    );
    if let Some(metrics) = &decision.metrics {
        line.push_str(&format!(
            " local_samples={} remote_samples={} local_prediction_ms={} remote_prediction_ms={} local_queue_ms={} remote_queue_ms={} local_completion_ms={} remote_completion_ms={} local_slots={} remote_slots={} local_active_count={} admitted_count={}",
            metrics.local_samples,
            metrics.remote_samples,
            metrics.local_prediction_ms,
            metrics.remote_prediction_ms,
            metrics.local_queue_ms,
            metrics.remote_queue_ms,
            metrics.local_completion_ms,
            metrics.remote_completion_ms,
            metrics.local_slots,
            metrics.remote_slots,
            metrics.local_active_count,
            metrics.admitted_count
        ));
    }
    line
}

fn quoted_log_value(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::types::{BuildCandidate, Decision};
    use crate::config::DEFAULT_REMOTE_HOST;
    use crate::persistence::events::record_event;
    use crate::persistence::open_history_db;
    use crate::test_support::{
        build_event, exploration_drv_path, insert_remote_admission, remote_admission_count,
        test_candidate, test_config, test_data_dir, write_remote_stats, write_remote_telemetry,
        write_remote_telemetry_full,
    };
    use std::fs;
    use std::path::PathBuf;

    const DEFAULT_MAX_REMOTE_ADMITTED: usize = 16;

    #[test]
    fn active_local_builds_make_idle_remote_preferable() {
        let dir = test_data_dir("decision-active-local");
        let cfg = test_config(dir.clone());
        write_remote_telemetry(&dir, now_ms(), 0.0);
        let observed_drv = "/nix/store/hash-kwin-6.6.3.drv";
        let active_drv = "/nix/store/hash-kwin-6.6.4.drv";
        record_event(&cfg, &build_event("start", observed_drv, 1_000, "unknown")).unwrap();
        record_event(
            &cfg,
            &build_event("finish", observed_drv, 1_801_000, "success"),
        )
        .unwrap();
        record_event(&cfg, &build_event("start", active_drv, now_ms(), "unknown")).unwrap();
        let candidate = BuildCandidate {
            am_willing: 1,
            needed_system: "x86_64-linux".to_string(),
            drv_path: "/nix/store/hash-kwin-6.6.5.drv".to_string(),
            required_features: Vec::new(),
            pname: "kwin".to_string(),
            remote_host: DEFAULT_REMOTE_HOST.to_string(),
            remote_store_uri: crate::config::DEFAULT_REMOTE_STORE_URI.to_string(),
        };

        let decision = decide_build_candidate(&cfg, &candidate).unwrap();
        assert_eq!(decision.decision, "accept");
        let metrics = decision.metrics.unwrap();
        assert_eq!(metrics.local_samples, 1);
        assert_eq!(metrics.remote_samples, 0);
        assert_eq!(metrics.local_active_count, 1);
        assert!(metrics.local_completion_ms > metrics.remote_completion_ms);
        assert_eq!(remote_admission_count(&dir), 1);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn exploration_can_refresh_unlucky_slow_local_sample() {
        let dir = test_data_dir("decision-explore-local");
        let cfg = test_config(dir.clone());
        write_remote_telemetry(&dir, now_ms(), 0.0);
        write_remote_stats(&dir, "kwin", 1, 10_000);
        let observed_drv = "/nix/store/hash-kwin-6.6.3.drv";
        record_event(&cfg, &build_event("start", observed_drv, 1_000, "unknown")).unwrap();
        record_event(
            &cfg,
            &build_event("finish", observed_drv, 1_801_000, "success"),
        )
        .unwrap();
        let candidate = BuildCandidate {
            am_willing: 1,
            needed_system: "x86_64-linux".to_string(),
            drv_path: exploration_drv_path("kwin-refresh-local"),
            required_features: Vec::new(),
            pname: "kwin".to_string(),
            remote_host: DEFAULT_REMOTE_HOST.to_string(),
            remote_store_uri: crate::config::DEFAULT_REMOTE_STORE_URI.to_string(),
        };

        let decision = decide_build_candidate(&cfg, &candidate).unwrap();
        assert_eq!(decision.decision, "decline");
        assert_eq!(decision.reason, "exploration: empty local host selected");
        let metrics = decision.metrics.unwrap();
        assert_eq!(metrics.local_samples, 1);
        assert_eq!(metrics.remote_samples, 1);
        assert!(metrics.remote_completion_ms < metrics.local_completion_ms);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn exploration_can_refresh_unlucky_slow_remote_sample() {
        let dir = test_data_dir("decision-explore-remote");
        let cfg = test_config(dir.clone());
        write_remote_telemetry(&dir, now_ms(), 0.0);
        write_remote_stats(&dir, "kwin", 1, 1_800_000);
        let observed_drv = "/nix/store/hash-kwin-6.6.3.drv";
        record_event(&cfg, &build_event("start", observed_drv, 1_000, "unknown")).unwrap();
        record_event(
            &cfg,
            &build_event("finish", observed_drv, 11_000, "success"),
        )
        .unwrap();
        let candidate = BuildCandidate {
            am_willing: 1,
            needed_system: "x86_64-linux".to_string(),
            drv_path: exploration_drv_path("kwin-refresh-remote"),
            required_features: Vec::new(),
            pname: "kwin".to_string(),
            remote_host: DEFAULT_REMOTE_HOST.to_string(),
            remote_store_uri: crate::config::DEFAULT_REMOTE_STORE_URI.to_string(),
        };

        let decision = decide_build_candidate(&cfg, &candidate).unwrap();
        assert_eq!(decision.decision, "accept");
        assert_eq!(decision.reason, "exploration: empty remote host selected");
        let metrics = decision.metrics.unwrap();
        assert_eq!(metrics.local_samples, 1);
        assert_eq!(metrics.remote_samples, 1);
        assert!(metrics.local_completion_ms < metrics.remote_completion_ms);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn stale_remote_telemetry_declines_decision() {
        let dir = test_data_dir("decision-stale");
        let cfg = test_config(dir.clone());
        write_remote_telemetry(&dir, 1, 0.0);
        let candidate = test_candidate("/nix/store/hash-kwin-6.6.3.drv");
        let decision = decide_build_candidate(&cfg, &candidate).unwrap();
        assert_eq!(decision.decision, "decline");
        assert_eq!(decision.reason, "remote telemetry is stale");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn busy_remote_telemetry_declines_decision() {
        let dir = test_data_dir("decision-busy");
        let cfg = test_config(dir.clone());
        write_remote_telemetry(&dir, now_ms(), 0.95);
        let candidate = test_candidate("/nix/store/hash-kwin-6.6.3.drv");
        let decision = decide_build_candidate(&cfg, &candidate).unwrap();
        assert_eq!(decision.decision, "decline");
        assert_eq!(decision.reason, "remote cpu is busy");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn memory_pressure_remote_telemetry_declines_decision() {
        let dir = test_data_dir("decision-memory-pressure");
        let cfg = test_config(dir.clone());
        write_remote_telemetry_full(&dir, now_ms(), 0.0, 64_000_000, 11.0, 0);
        let candidate = test_candidate("/nix/store/hash-kwin-6.6.3.drv");
        let decision = decide_build_candidate(&cfg, &candidate).unwrap();
        assert_eq!(decision.decision, "decline");
        assert_eq!(decision.reason, "remote memory pressure is high");
        assert_eq!(remote_admission_count(&dir), 0);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn low_memory_remote_telemetry_declines_decision() {
        let dir = test_data_dir("decision-low-memory");
        let cfg = test_config(dir.clone());
        write_remote_telemetry_full(&dir, now_ms(), 0.0, 1024 * 1024, 0.0, 0);
        let candidate = test_candidate("/nix/store/hash-kwin-6.6.3.drv");
        let decision = decide_build_candidate(&cfg, &candidate).unwrap();
        assert_eq!(decision.decision, "decline");
        assert_eq!(decision.reason, "remote memory is low");
        assert_eq!(remote_admission_count(&dir), 0);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn remote_admission_limit_declines_decision() {
        let dir = test_data_dir("decision-admission-limit");
        let cfg = test_config(dir.clone());
        write_remote_telemetry(&dir, now_ms(), 0.0);
        let conn = open_history_db(&dir).unwrap();
        let admitted_at_ms = now_ms().saturating_sub(2_000);
        for idx in 0..DEFAULT_MAX_REMOTE_ADMITTED {
            insert_remote_admission(
                &conn,
                &format!("/nix/store/hash-admitted-{idx}.drv"),
                admitted_at_ms,
                10_000,
                false,
            );
        }
        drop(conn);
        let candidate = test_candidate("/nix/store/hash-kwin-6.6.3.drv");

        let decision = decide_build_candidate(&cfg, &candidate).unwrap();
        assert_eq!(decision.decision, "decline");
        assert_eq!(decision.reason, "remote admission limit reached");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn unknown_remote_admission_limit_declines_decision() {
        let dir = test_data_dir("decision-unknown-admission-limit");
        let cfg = test_config(dir.clone());
        write_remote_telemetry(&dir, now_ms(), 0.0);
        let conn = open_history_db(&dir).unwrap();
        insert_remote_admission(
            &conn,
            "/nix/store/hash-unknown-admitted.drv",
            now_ms().saturating_sub(2_000),
            10_000,
            true,
        );
        drop(conn);
        let candidate = test_candidate("/nix/store/hash-kwin-6.6.3.drv");

        let decision = decide_build_candidate(&cfg, &candidate).unwrap();
        assert_eq!(decision.decision, "decline");
        assert_eq!(decision.reason, "unknown remote admission limit reached");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn recent_remote_admission_declines_decision() {
        let dir = test_data_dir("decision-admission-interval");
        let cfg = test_config(dir.clone());
        write_remote_telemetry(&dir, now_ms(), 0.0);
        let conn = open_history_db(&dir).unwrap();
        insert_remote_admission(
            &conn,
            "/nix/store/hash-recent-admitted.drv",
            now_ms(),
            10_000,
            false,
        );
        drop(conn);
        let candidate = test_candidate("/nix/store/hash-kwin-6.6.3.drv");

        let decision = decide_build_candidate(&cfg, &candidate).unwrap();
        assert_eq!(decision.decision, "decline");
        assert_eq!(decision.reason, "remote admission interval not elapsed");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn scheduler_decision_log_includes_candidate_and_reason() {
        let cfg = test_config(PathBuf::from("/tmp/unused"));
        let candidate = BuildCandidate {
            am_willing: 1,
            needed_system: "x86_64-linux".to_string(),
            drv_path: "/nix/store/hash-quoted\"package-1.0.drv".to_string(),
            required_features: vec!["kvm".to_string(), "big-parallel".to_string()],
            pname: "quoted\"package".to_string(),
            remote_host: DEFAULT_REMOTE_HOST.to_string(),
            remote_store_uri: crate::config::DEFAULT_REMOTE_STORE_URI.to_string(),
        };
        let decision = Decision {
            decision: "decline".to_string(),
            reason: "remote cpu is busy".to_string(),
            store_uri: None,
            metrics: None,
        };

        let line = scheduler_decision_log_line(&cfg, &candidate, &decision);
        assert!(line.contains("scheduler_decision "));
        assert!(line.contains("remote_host=\"tsugumi\""));
        assert!(line.contains("decision=\"decline\""));
        assert!(line.contains("reason=\"remote cpu is busy\""));
        assert!(line.contains("required_features=\"kvm,big-parallel\""));
        assert!(line.contains("quoted\\\"package"));
        assert!(line.contains("store_uri=null"));
    }
}
