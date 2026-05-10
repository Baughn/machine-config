use serde::{Deserialize, Serialize};

use crate::config::{
    default_needed_system, default_remote_host, default_remote_store_uri, default_unknown,
};
use crate::util::{hostname_fallback, now_ms};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Telemetry {
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub timestamp_ms: u128,
    pub cpu_busy_ratio: Option<f64>,
    pub mem_total_kb: Option<u64>,
    pub mem_available_kb: Option<u64>,
    pub psi_memory_some_avg10: Option<f64>,
    #[serde(default)]
    pub nix_slots_total: usize,
    #[serde(default)]
    pub nix_slots_local: usize,
    #[serde(default)]
    pub nix_slots_remote: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BuildEvent {
    #[serde(default = "default_unknown")]
    pub kind: String,
    #[serde(default)]
    pub drv_path: String,
    #[serde(default)]
    pub out_paths: String,
    #[serde(default = "default_unknown")]
    pub status: String,
    #[serde(default = "hostname_fallback")]
    pub host: String,
    #[serde(default = "now_ms")]
    pub timestamp_ms: u128,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BuildCandidate {
    #[serde(default)]
    pub am_willing: u64,
    #[serde(default = "default_needed_system")]
    pub needed_system: String,
    pub drv_path: String,
    #[serde(default)]
    pub required_features: Vec<String>,
    #[serde(default)]
    pub pname: String,
    #[serde(default = "default_remote_host")]
    pub remote_host: String,
    #[serde(default = "default_remote_store_uri")]
    pub remote_store_uri: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Decision {
    pub decision: String,
    pub reason: String,
    pub store_uri: Option<String>,
    #[serde(skip_serializing, default)]
    pub metrics: Option<DecisionMetrics>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DecisionMetrics {
    pub local_samples: u64,
    pub remote_samples: u64,
    pub local_prediction_ms: u64,
    pub remote_prediction_ms: u64,
    pub local_queue_ms: u64,
    pub remote_queue_ms: u64,
    pub local_completion_ms: u64,
    pub remote_completion_ms: u64,
    pub local_slots: usize,
    pub remote_slots: usize,
    pub local_active_count: usize,
    pub admitted_count: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PackageStats {
    pub count: u64,
    pub p95_ms: u64,
}

#[derive(Serialize, Deserialize)]
pub struct StatsResponse {
    pub unknown_p95_ms: u64,
    pub packages: Vec<PackageStatsEntry>,
}

#[derive(Serialize, Deserialize)]
pub struct PackageStatsEntry {
    pub pname: String,
    pub count: u64,
    pub p50_ms: u64,
    pub p80_ms: u64,
    pub p95_ms: u64,
}

pub fn json_line<T: Serialize>(value: &T) -> String {
    let mut out = serde_json::to_string(value).unwrap_or_else(|err| {
        format!(
            "{{\"error\":{}}}",
            serde_json::to_string(&err.to_string())
                .unwrap_or_else(|_| "\"serialization failed\"".to_string())
        )
    });
    out.push('\n');
    out
}

pub fn telemetry_json(telemetry: &Telemetry) -> String {
    json_line(telemetry)
}

pub fn event_body(event: &BuildEvent) -> String {
    json_line(event)
}
