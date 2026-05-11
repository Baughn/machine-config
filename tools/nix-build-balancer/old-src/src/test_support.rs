#![cfg(test)]

use rusqlite::{params, Connection};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::api::types::{BuildCandidate, BuildEvent, DecisionMetrics};
use crate::config::{Config, Mode, DEFAULT_REMOTE_HOST, DEFAULT_REMOTE_STORE_URI};
use crate::persistence::open_history_db;
use crate::scheduler::eligibility::stable_percent;
use crate::scheduler::policy::{
    BuildTarget, DEFAULT_EXPLORATION_MIN_SAMPLES, DEFAULT_EXPLORATION_PERCENT,
    DEFAULT_REMOTE_CAPACITY,
};
use crate::util::{duration_to_i64, now_ms, pname_from_drv, timestamp_to_i64};

pub fn test_data_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "nix-build-balancer-test-{name}-{}-{}",
        std::process::id(),
        now_ms()
    ));
    let _ = fs::remove_dir_all(&dir);
    dir
}

pub fn test_config(data_dir: PathBuf) -> Config {
    Config {
        mode: Mode::Agent,
        host: "test-host".to_string(),
        data_dir,
        unix_socket: None,
        listen: None,
        remote: Vec::new(),
        poll_interval: Duration::from_secs(1),
        max_samples_per_pname: 200,
        stale_start_ms: 0,
        once: true,
    }
}

pub fn write_remote_telemetry(dir: &Path, timestamp_ms: u128, cpu_busy_ratio: f64) {
    write_remote_telemetry_full(dir, timestamp_ms, cpu_busy_ratio, 64_000_000, 0.0, 0);
}

pub fn write_remote_telemetry_full(
    dir: &Path,
    timestamp_ms: u128,
    cpu_busy_ratio: f64,
    mem_available_kb: u64,
    psi_memory_some_avg10: f64,
    nix_slots_total: usize,
) {
    fs::create_dir_all(dir).unwrap();
    fs::write(
        dir.join("telemetry-tsugumi.json"),
        format!(
            "{{\"host\":\"tsugumi\",\"timestamp_ms\":{},\"cpu_busy_ratio\":{},\"mem_total_kb\":130000000,\"mem_available_kb\":{},\"psi_memory_some_avg10\":{},\"nix_slots_total\":{},\"nix_slots_local\":0,\"nix_slots_remote\":0}}\n",
            timestamp_ms,
            cpu_busy_ratio,
            mem_available_kb,
            psi_memory_some_avg10,
            nix_slots_total
        ),
    )
    .unwrap();
}

pub fn write_remote_stats(dir: &Path, pname: &str, count: u64, p95_ms: u64) {
    fs::create_dir_all(dir).unwrap();
    fs::write(
        dir.join("stats-tsugumi.json"),
        format!(
            "{{\"unknown_p95_ms\":1800000,\"packages\":[{{\"pname\":{},\"count\":{},\"p50_ms\":{},\"p80_ms\":{},\"p95_ms\":{}}}]}}\n",
            serde_json::to_string(pname).unwrap(),
            count,
            p95_ms,
            p95_ms,
            p95_ms
        ),
    )
    .unwrap();
}

pub fn exploration_drv_path(prefix: &str) -> String {
    (0..1_000)
        .map(|idx| format!("/nix/store/hash-{prefix}-{idx}.drv"))
        .find(|path| stable_percent(path) < DEFAULT_EXPLORATION_PERCENT)
        .expect("test should find an exploration bucket")
}

pub fn non_exploration_drv_path(prefix: &str) -> String {
    (0..1_000)
        .map(|idx| format!("/nix/store/hash-{prefix}-{idx}.drv"))
        .find(|path| stable_percent(path) >= DEFAULT_EXPLORATION_PERCENT)
        .expect("test should find a non-exploration bucket")
}

pub fn build_event(kind: &str, drv_path: &str, timestamp_ms: u128, status: &str) -> BuildEvent {
    BuildEvent {
        kind: kind.to_string(),
        drv_path: drv_path.to_string(),
        out_paths: "/nix/store/out".to_string(),
        status: status.to_string(),
        host: "test-host".to_string(),
        timestamp_ms,
    }
}

pub fn test_candidate(drv_path: &str) -> BuildCandidate {
    BuildCandidate {
        am_willing: 1,
        needed_system: "x86_64-linux".to_string(),
        drv_path: drv_path.to_string(),
        required_features: Vec::new(),
        pname: pname_from_drv(drv_path),
        remote_host: DEFAULT_REMOTE_HOST.to_string(),
        remote_store_uri: DEFAULT_REMOTE_STORE_URI.to_string(),
    }
}

pub fn test_target() -> BuildTarget {
    BuildTarget {
        host_name: DEFAULT_REMOTE_HOST.to_string(),
        store_uri: DEFAULT_REMOTE_STORE_URI.to_string(),
        capacity: DEFAULT_REMOTE_CAPACITY,
    }
}

pub fn test_metrics(local_completion_ms: u64, remote_completion_ms: u64) -> DecisionMetrics {
    DecisionMetrics {
        local_samples: DEFAULT_EXPLORATION_MIN_SAMPLES,
        remote_samples: DEFAULT_EXPLORATION_MIN_SAMPLES,
        local_prediction_ms: local_completion_ms,
        remote_prediction_ms: remote_completion_ms,
        local_queue_ms: 0,
        remote_queue_ms: 0,
        local_completion_ms,
        remote_completion_ms,
        local_slots: 0,
        remote_slots: 0,
        local_active_count: 0,
        admitted_count: 0,
    }
}

pub fn insert_remote_admission(
    conn: &Connection,
    drv_path: &str,
    admitted_at_ms: u128,
    predicted_ms: u64,
    unknown: bool,
) {
    conn.execute(
        "INSERT INTO remote_admissions
           (drv_path, host, pname, admitted_at_ms, predicted_ms, unknown)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            drv_path,
            DEFAULT_REMOTE_HOST,
            pname_from_drv(drv_path),
            timestamp_to_i64(admitted_at_ms).unwrap(),
            duration_to_i64(predicted_ms as u128).unwrap(),
            if unknown { 1 } else { 0 },
        ],
    )
    .unwrap();
}

pub fn remote_admission_count(dir: &Path) -> i64 {
    let conn = open_history_db(dir).unwrap();
    conn.query_row("SELECT count(*) FROM remote_admissions", [], |row| {
        row.get(0)
    })
    .unwrap()
}
