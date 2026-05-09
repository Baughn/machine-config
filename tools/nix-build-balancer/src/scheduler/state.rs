use crate::api::types::{Decision, PackageStats, Telemetry};
use crate::persistence::queries::Admission;

#[derive(Debug)]
pub struct HostState {
    pub telemetry: Telemetry,
    pub stats: Option<PackageStats>,
    pub active_count: usize,
    pub active_queue_ms: u64,
    pub admissions: Vec<Admission>,
}

#[derive(Debug)]
pub struct Prediction {
    pub samples: u64,
    pub package_ms: u64,
    pub queue_ms: u64,
    pub completion_ms: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Eligibility {
    Accepted,
    Declined { reason: &'static str },
}

#[derive(Debug)]
pub struct DecisionOutcome {
    pub decision: Decision,
    pub record_remote_admission: bool,
}
