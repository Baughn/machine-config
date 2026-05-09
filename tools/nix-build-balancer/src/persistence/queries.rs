use rusqlite::{params, Connection};
use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use crate::api::types::{json_line, PackageStats, PackageStatsEntry, StatsResponse};
use crate::scheduler::policy::{SchedulerPolicy, DEFAULT_UNKNOWN_P95_MS};
use crate::util::sqlite_error;

/// Read local successful build durations for one package and return p95 stats.
pub fn local_package_stats_from_conn(
    conn: &Connection,
    pname: &str,
) -> io::Result<Option<PackageStats>> {
    let mut stmt = conn
        .prepare(
            "SELECT duration_ms FROM build_observations
             WHERE status = 'success' AND pname = ?1
             ORDER BY duration_ms",
        )
        .map_err(sqlite_error)?;
    let rows = stmt
        .query_map(params![pname], |row| row.get::<_, i64>(0))
        .map_err(sqlite_error)?;
    let mut values = Vec::new();
    for row in rows {
        let duration = row.map_err(sqlite_error)?;
        if duration >= 0 {
            values.push(duration as u64);
        }
    }
    Ok((!values.is_empty()).then(|| PackageStats {
        count: values.len() as u64,
        p95_ms: quantile(&values, 0.95).unwrap_or(DEFAULT_UNKNOWN_P95_MS),
    }))
}

/// Estimate remaining local work from active build starts recorded in SQLite.
pub fn active_local_queue_ms(
    conn: &Connection,
    host: &str,
    now: u128,
    policy: &SchedulerPolicy,
) -> io::Result<(usize, u64)> {
    let mut stmt = conn
        .prepare(
            "SELECT pname, started_at_ms FROM active_builds
             WHERE host = ?1",
        )
        .map_err(sqlite_error)?;
    let rows = stmt
        .query_map(params![host], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(sqlite_error)?;
    let mut active = Vec::new();
    for row in rows {
        active.push(row.map_err(sqlite_error)?);
    }
    drop(stmt);

    let mut remaining_ms = 0u64;
    let active_count = active.len();
    for (pname, started_at_ms) in active {
        let stats = local_package_stats_from_conn(conn, &pname)?;
        let prediction = sample_prediction_ms(stats.as_ref()).unwrap_or(policy.unknown_p95_ms);
        let elapsed_ms = now.saturating_sub(started_at_ms.max(0) as u128) as u64;
        remaining_ms = remaining_ms.saturating_add(prediction.saturating_sub(elapsed_ms));
    }

    Ok((active_count, remaining_ms / policy.local_capacity as u64))
}

#[derive(Debug)]
pub struct Admission {
    pub admitted_at_ms: i64,
    pub predicted_ms: u64,
    pub unknown: bool,
}

/// Load active remote admissions for one host in admission order.
pub fn remote_admissions(conn: &Connection, remote_host: &str) -> io::Result<Vec<Admission>> {
    let mut stmt = conn
        .prepare(
            "SELECT admitted_at_ms, predicted_ms, unknown FROM remote_admissions
             WHERE host = ?1
             ORDER BY admitted_at_ms",
        )
        .map_err(sqlite_error)?;
    let rows = stmt
        .query_map(params![remote_host], |row| {
            let predicted_ms = row.get::<_, i64>(1)?.max(0) as u64;
            Ok(Admission {
                admitted_at_ms: row.get(0)?,
                predicted_ms,
                unknown: row.get::<_, i64>(2)? != 0,
            })
        })
        .map_err(sqlite_error)?;
    let mut admissions = Vec::new();
    for row in rows {
        admissions.push(row.map_err(sqlite_error)?);
    }
    Ok(admissions)
}

/// Render local successful build history as the remote-agent `/stats` response.
pub fn stats_json(data_dir: &Path) -> io::Result<String> {
    let conn = super::open_history_db(data_dir)?;
    let mut durations: BTreeMap<String, Vec<u64>> = BTreeMap::new();
    let mut stmt = conn
        .prepare(
            "SELECT pname, duration_ms FROM build_observations
             WHERE status = 'success'
             ORDER BY pname",
        )
        .map_err(sqlite_error)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(sqlite_error)?;
    for row in rows {
        let (pname, duration) = row.map_err(sqlite_error)?;
        if duration >= 0 {
            durations.entry(pname).or_default().push(duration as u64);
        }
    }

    let mut packages = Vec::new();
    for (pname, mut values) in durations {
        values.sort_unstable();
        let count = values.len();
        packages.push(PackageStatsEntry {
            pname,
            count: count as u64,
            p50_ms: quantile(&values, 0.50).unwrap_or(DEFAULT_UNKNOWN_P95_MS),
            p80_ms: quantile(&values, 0.80).unwrap_or(DEFAULT_UNKNOWN_P95_MS),
            p95_ms: quantile(&values, 0.95).unwrap_or(DEFAULT_UNKNOWN_P95_MS),
        });
    }

    Ok(json_line(&StatsResponse {
        unknown_p95_ms: DEFAULT_UNKNOWN_P95_MS,
        packages,
    }))
}

/// Return the upper-bucket quantile for already sorted duration values.
pub fn quantile(values: &[u64], q: f64) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let idx = ((values.len() - 1) as f64 * q).ceil() as usize;
    values.get(idx).copied()
}

/// Convert package stats into a non-zero duration prediction.
pub fn sample_prediction_ms(stats: Option<&PackageStats>) -> Option<u64> {
    stats.and_then(|stats| (stats.count > 0).then(|| stats.p95_ms.max(1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantiles_use_upper_bucket() {
        let values = [10, 20, 30, 40];
        assert_eq!(quantile(&values, 0.50), Some(30));
        assert_eq!(quantile(&values, 0.95), Some(40));
    }
}
