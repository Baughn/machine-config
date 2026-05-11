use crate::api::types::BuildCandidate;
use crate::config::Config;

pub const DEFAULT_UNKNOWN_P95_MS: u64 = 30 * 60 * 1000;
const DEFAULT_STALE_TELEMETRY_MS: u128 = 10_000;
const DEFAULT_MAX_REMOTE_ADMITTED: usize = 16;
const DEFAULT_MAX_UNKNOWN_REMOTE: usize = 1;
const DEFAULT_MIN_REMOTE_ADMISSION_INTERVAL_MS: u128 = 1_000;
pub const DEFAULT_EXPLORATION_PERCENT: u64 = 20;
pub const DEFAULT_EXPLORATION_MIN_SAMPLES: u64 = 4;
const DEFAULT_LOCAL_CAPACITY: usize = 32;
pub const DEFAULT_REMOTE_CAPACITY: usize = 16;

#[derive(Debug)]
pub struct SchedulerConfig {
    pub local_host_name: String,
    pub remote_target: BuildTarget,
    pub policy: SchedulerPolicy,
}

#[derive(Clone, Debug)]
pub struct SchedulerPolicy {
    pub unknown_p95_ms: u64,
    pub stale_telemetry_ms: u128,
    pub max_remote_admitted: usize,
    pub max_unknown_remote: usize,
    pub min_remote_admission_interval_ms: u128,
    pub exploration_percent: u64,
    pub exploration_min_samples: u64,
    pub local_capacity: usize,
    pub remote_capacity: usize,
    pub max_remote_cpu_busy_ratio: f64,
    pub max_remote_memory_pressure_avg10: f64,
    pub min_remote_mem_available_kb: u64,
}

#[derive(Debug)]
pub struct BuildTarget {
    pub host_name: String,
    pub store_uri: String,
    pub capacity: usize,
}

impl SchedulerConfig {
    pub fn from_candidate(cfg: &Config, candidate: &BuildCandidate) -> Self {
        let policy = SchedulerPolicy::default();
        Self {
            local_host_name: cfg.host.clone(),
            remote_target: BuildTarget::from_candidate(candidate, &policy),
            policy,
        }
    }
}

impl Default for SchedulerPolicy {
    fn default() -> Self {
        Self {
            unknown_p95_ms: DEFAULT_UNKNOWN_P95_MS,
            stale_telemetry_ms: DEFAULT_STALE_TELEMETRY_MS,
            max_remote_admitted: DEFAULT_MAX_REMOTE_ADMITTED,
            max_unknown_remote: DEFAULT_MAX_UNKNOWN_REMOTE,
            min_remote_admission_interval_ms: DEFAULT_MIN_REMOTE_ADMISSION_INTERVAL_MS,
            exploration_percent: DEFAULT_EXPLORATION_PERCENT,
            exploration_min_samples: DEFAULT_EXPLORATION_MIN_SAMPLES,
            local_capacity: DEFAULT_LOCAL_CAPACITY,
            remote_capacity: DEFAULT_REMOTE_CAPACITY,
            max_remote_cpu_busy_ratio: 0.90,
            max_remote_memory_pressure_avg10: 10.0,
            min_remote_mem_available_kb: 4 * 1024 * 1024,
        }
    }
}

impl BuildTarget {
    pub fn from_candidate(candidate: &BuildCandidate, policy: &SchedulerPolicy) -> Self {
        Self {
            host_name: candidate.remote_host.clone(),
            store_uri: candidate.remote_store_uri.clone(),
            capacity: policy.remote_capacity,
        }
    }
}
