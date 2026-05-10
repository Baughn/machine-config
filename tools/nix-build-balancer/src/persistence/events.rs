use rusqlite::{params, OptionalExtension};
use std::io;
use std::path::Path;

use crate::api::types::{BuildCandidate, BuildEvent};
use crate::config::Config;
use crate::util::{duration_to_i64, pname_from_drv, sqlite_error, timestamp_to_i64};

use super::cleanup::{cleanup_stale_starts, prune_pname_samples};
use super::open_history_db;

pub fn record_event(cfg: &Config, event: &BuildEvent) -> io::Result<()> {
    let mut conn = open_history_db(&cfg.data_dir)?;
    cleanup_stale_starts(&conn, cfg.stale_start_ms)?;

    let pname = pname_from_drv(&event.drv_path);
    if event.kind == "start" {
        conn.execute(
            "INSERT INTO active_builds (drv_path, host, pname, started_at_ms)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(drv_path) DO UPDATE SET
               host = excluded.host,
               pname = excluded.pname,
               started_at_ms = excluded.started_at_ms",
            params![
                event.drv_path,
                event.host,
                pname,
                timestamp_to_i64(event.timestamp_ms)?
            ],
        )
        .map_err(sqlite_error)?;
    } else if event.kind == "finish" {
        let tx = conn.transaction().map_err(sqlite_error)?;
        let start = tx
            .query_row(
                "SELECT host, pname, started_at_ms FROM active_builds WHERE drv_path = ?1",
                params![event.drv_path],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(sqlite_error)?;

        tx.execute(
            "DELETE FROM active_builds WHERE drv_path = ?1",
            params![event.drv_path],
        )
        .map_err(sqlite_error)?;

        if let Some((start_host, start_pname, start_ms)) = start {
            let start_ms_u128 = start_ms.max(0) as u128;
            let duration_ms = event.timestamp_ms.saturating_sub(start_ms_u128);
            tx.execute(
                "INSERT INTO build_observations
                   (host, pname, drv_path, started_at_ms, finished_at_ms, duration_ms, status, out_paths)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    start_host,
                    start_pname,
                    event.drv_path,
                    start_ms,
                    timestamp_to_i64(event.timestamp_ms)?,
                    duration_to_i64(duration_ms)?,
                    event.status,
                    event.out_paths,
                ],
            )
            .map_err(sqlite_error)?;
            prune_pname_samples(&tx, &start_pname, cfg.max_samples_per_pname)?;
            tracing::info!(
                target: "build_finished",
                host = %event.host,
                pname = %pname,
                duration_ms,
                status = %event.status,
                "build_finished",
            );
        } else {
            tracing::info!(
                target: "build_finish_unmatched",
                host = %event.host,
                pname = %pname,
                status = %event.status,
                "build_finish_unmatched",
            );
        }
        tx.commit().map_err(sqlite_error)?;
    }
    Ok(())
}

/// Record an accepted remote decision until the hook reports completion.
pub fn record_remote_admission_at(
    conn: &rusqlite::Connection,
    candidate: &BuildCandidate,
    remote_prediction: u64,
    unknown: bool,
    admitted_at_ms: u128,
) -> io::Result<()> {
    conn.execute(
        "INSERT INTO remote_admissions
           (drv_path, host, pname, admitted_at_ms, predicted_ms, unknown)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(drv_path) DO UPDATE SET
           host = excluded.host,
           pname = excluded.pname,
           admitted_at_ms = excluded.admitted_at_ms,
           predicted_ms = excluded.predicted_ms,
           unknown = excluded.unknown",
        params![
            candidate.drv_path,
            candidate.remote_host,
            candidate.pname,
            timestamp_to_i64(admitted_at_ms)?,
            duration_to_i64(remote_prediction as u128)?,
            if unknown { 1 } else { 0 },
        ],
    )
    .map_err(sqlite_error)?;
    Ok(())
}

/// Mark a previously admitted remote derivation as no longer queued or running.
pub fn finish_admission(data_dir: &Path, drv_path: &str) -> io::Result<()> {
    let conn = open_history_db(data_dir)?;
    conn.execute(
        "DELETE FROM remote_admissions WHERE drv_path = ?1",
        params![drv_path],
    )
    .map_err(sqlite_error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::queries::stats_json;
    use crate::test_support::{build_event, test_config, test_data_dir};
    use std::fs;

    #[test]
    fn records_successful_build_stats_from_sqlite() {
        let dir = test_data_dir("stats");
        let cfg = test_config(dir.clone());
        let drv = "/nix/store/hash-kwin-6.6.3.drv";

        record_event(&cfg, &build_event("start", drv, 1_000, "unknown")).unwrap();
        record_event(&cfg, &build_event("finish", drv, 2_500, "success")).unwrap();

        let stats = stats_json(&dir).unwrap();
        assert!(stats.contains("\"pname\":\"kwin\""));
        assert!(stats.contains("\"count\":1"));
        assert!(stats.contains("\"p50_ms\":1500"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn stores_failures_but_excludes_them_from_stats() {
        let dir = test_data_dir("failure");
        let cfg = test_config(dir.clone());
        let drv = "/nix/store/hash-failing-package-1.0.drv";

        record_event(&cfg, &build_event("start", drv, 1_000, "unknown")).unwrap();
        record_event(&cfg, &build_event("finish", drv, 2_000, "failure")).unwrap();

        let conn = open_history_db(&dir).unwrap();
        let count: i64 = conn
            .query_row("SELECT count(*) FROM build_observations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
        let stats = stats_json(&dir).unwrap();
        assert!(!stats.contains("failing-package"));
        let _ = fs::remove_dir_all(dir);
    }
}
