use rusqlite::Connection;
use std::io;

use crate::api::types::{BuildCandidate, Telemetry};
use crate::config::Config;
use crate::persistence::queries::{
    active_local_queue_ms, local_package_stats_from_conn, remote_admissions,
};
use crate::scheduler::policy::{BuildTarget, SchedulerConfig};
use crate::scheduler::state::HostState;
use crate::telemetry::{read_remote_telemetry, remote_package_stats};

/// Load live local telemetry and local package history for a candidate.
pub fn load_local_host_state(
    conn: &Connection,
    candidate: &BuildCandidate,
    scheduler: &SchedulerConfig,
    telemetry: Telemetry,
    now: u128,
) -> io::Result<HostState> {
    let stats = local_package_stats_from_conn(conn, &candidate.pname)?;
    let (active_count, active_queue_ms) =
        active_local_queue_ms(conn, &scheduler.local_host_name, now, &scheduler.policy)?;
    Ok(HostState {
        telemetry,
        stats,
        active_count,
        active_queue_ms,
        admissions: Vec::new(),
    })
}

/// Load cached remote telemetry, cached package stats, and active admissions.
pub fn load_remote_host_state(
    conn: &Connection,
    cfg: &Config,
    candidate: &BuildCandidate,
    target: &BuildTarget,
) -> io::Result<HostState> {
    let telemetry = read_remote_telemetry(&cfg.data_dir, &target.host_name)?;
    let stats = remote_package_stats(&cfg.data_dir, &target.host_name, &candidate.pname)?;
    let admissions = remote_admissions(conn, &target.host_name)?;
    Ok(HostState {
        telemetry,
        stats,
        active_count: 0,
        active_queue_ms: 0,
        admissions,
    })
}
