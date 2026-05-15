use std::io;

use rusqlite::{params, Connection};

use crate::estimator;
use crate::protocol::ops::EventBuildFinish;

/// Insert one row into `build_observations`, then trim the per-pname history
/// to `max_samples_per_pname` newest rows (no-op when 0).
///
/// Returns `Ok(false)` and skips the insert when the event has no
/// `duration_ms` — spec §"Build observation lifecycle" item 5: rows are only
/// written when the duration is known. The caller (controller) still retires
/// the admission either way.
pub fn record_finish(
    conn: &Connection,
    event: &EventBuildFinish,
    max_samples_per_pname: u32,
) -> io::Result<bool> {
    let Some(duration_ms) = event.duration_ms else {
        return Ok(false);
    };
    let started_at_ms = event.ts_ms.saturating_sub(duration_ms);
    let out_paths = event.out_paths.join("\n");

    let inserted = conn
        .execute(
            "INSERT OR IGNORE INTO build_observations
             (host, pname, drv_path, started_at_ms, finished_at_ms, duration_ms, status, out_paths)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                &event.host,
                &event.pname,
                &event.drv_path,
                started_at_ms as i64,
                event.ts_ms as i64,
                duration_ms as i64,
                event.status.as_str(),
                out_paths,
            ],
        )
        .map_err(io::Error::other)?;

    if inserted == 0 {
        // Duplicate (drv_path, finished_at_ms) — same finish replayed
        // (e.g. controller restart, agent reconnects and re-pushes a
        // spool entry whose TCP write hadn't been confirmed). Skip the
        // prune so we don't drop a row counted toward our cap based on a
        // no-op insert.
        return Ok(false);
    }

    if max_samples_per_pname > 0 {
        prune_pname(conn, &event.pname, max_samples_per_pname)?;
    }
    Ok(true)
}

fn prune_pname(conn: &Connection, pname: &str, keep_newest: u32) -> io::Result<()> {
    conn.execute(
        "DELETE FROM build_observations
         WHERE rowid IN (
           SELECT rowid FROM build_observations
           WHERE pname = ?1
           ORDER BY finished_at_ms DESC, rowid DESC
           LIMIT -1 OFFSET ?2
         )",
        params![pname, keep_newest as i64],
    )
    .map_err(io::Error::other)?;
    Ok(())
}

/// Conservative duration estimate for `pname`, fed straight into the
/// scheduler as `package_ms` and into the admission row as `predicted_ms`.
///
/// Reads every successful observation for `pname` in chronological order
/// (oldest first — order matters for the EWMA recurrence) and delegates
/// the arithmetic to [`estimator::predict_lognormal_ms`]. See that module
/// for the model, the references, and the rationale for picking the
/// upper-95 % quantile of a fitted log-normal over the unweighted sample
/// p95 it replaced.
///
/// Returns `None` when there are no successful rows, so the caller falls
/// back to the policy-level `unknown_p95_ms`.
pub fn predict_ms(conn: &Connection, pname: &str, alpha: f64, z: f64) -> io::Result<Option<u64>> {
    let mut stmt = conn
        .prepare(
            "SELECT duration_ms FROM build_observations
             WHERE status = 'success' AND pname = ?1
             ORDER BY finished_at_ms ASC, rowid ASC",
        )
        .map_err(io::Error::other)?;
    let rows = stmt
        .query_map(params![pname], |row| row.get::<_, i64>(0))
        .map_err(io::Error::other)?;
    let mut values = Vec::new();
    for row in rows {
        let duration = row.map_err(io::Error::other)?;
        if duration > 0 {
            values.push(duration as u64);
        }
    }
    Ok(estimator::predict_lognormal_ms(
        &values,
        alpha,
        z,
        estimator::MIN_LN_VAR,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::open_in_memory;
    use crate::protocol::ops::BuildStatus;

    fn finish(pname: &str, duration_ms: u64, status: BuildStatus, ts_ms: u64) -> EventBuildFinish {
        EventBuildFinish {
            drv_path: format!("/nix/store/aaa-{pname}-{ts_ms}.drv"),
            pname: pname.to_string(),
            host: "tsugumi".to_string(),
            ts_ms,
            duration_ms: Some(duration_ms),
            status,
            out_paths: vec![format!("/nix/store/yyy-{pname}")],
        }
    }

    const ALPHA: f64 = estimator::ALPHA_DEFAULT;
    const Z: f64 = estimator::Z_P95;

    #[test]
    fn predict_ms_none_on_empty_history() {
        let conn = open_in_memory().unwrap();
        assert_eq!(predict_ms(&conn, "foo", ALPHA, Z).unwrap(), None);
    }

    #[test]
    fn predict_ms_ignores_failure_rows() {
        // Two rows: one success, one failure. The failure row must not be
        // visible to the estimator — only the success row contributes.
        let conn = open_in_memory().unwrap();
        record_finish(&conn, &finish("foo", 5_000, BuildStatus::Success, 100), 0).unwrap();
        record_finish(&conn, &finish("foo", 999_000, BuildStatus::Failure, 200), 0).unwrap();
        // With one (positive) success sample the estimator returns it
        // verbatim — see [`estimator::predict_lognormal_ms`] for why a
        // single sample short-circuits the variance floor.
        assert_eq!(predict_ms(&conn, "foo", ALPHA, Z).unwrap(), Some(5_000));
    }

    #[test]
    fn predict_ms_reads_rows_in_chronological_order() {
        // Insert two batches in arbitrary insert order but with distinct
        // `finished_at_ms`. The SQL clause orders by `finished_at_ms ASC`
        // so the EWMA sees the older batch first regardless of insert
        // sequence. We verify by comparing against the same series fed
        // directly to the estimator.
        let conn = open_in_memory().unwrap();
        // Recent: 90s. Old: 144s. Insert recent first so SQL has to do the
        // work.
        record_finish(
            &conn,
            &finish("foo", 90_000, BuildStatus::Success, 2_000),
            0,
        )
        .unwrap();
        record_finish(
            &conn,
            &finish("foo", 144_000, BuildStatus::Success, 1_000),
            0,
        )
        .unwrap();
        let got = predict_ms(&conn, "foo", ALPHA, Z).unwrap().unwrap();
        let direct =
            estimator::predict_lognormal_ms(&[144_000, 90_000], ALPHA, Z, estimator::MIN_LN_VAR)
                .unwrap();
        assert_eq!(got, direct);
    }

    #[test]
    fn predict_ms_adapts_to_step_change_after_recovery_window() {
        // 100 historical builds at 40 minutes, then 30 new builds at
        // 30 minutes. After the recovery window the EW variance has
        // decayed and the mean has converged, so the prediction settles
        // near `1_800_000 × exp(z·√MIN_LN_VAR) ≈ 2.43e6` ms — well below
        // the *old* steady-state estimate of ≈ 3.24e6 ms. See
        // [`estimator::predict_lognormal_ms`] tests for the transient.
        let conn = open_in_memory().unwrap();
        for i in 0..100u64 {
            record_finish(
                &conn,
                &finish("foo", 2_400_000, BuildStatus::Success, 1_000 + i),
                0,
            )
            .unwrap();
        }
        for i in 0..30u64 {
            record_finish(
                &conn,
                &finish("foo", 1_800_000, BuildStatus::Success, 10_000 + i),
                0,
            )
            .unwrap();
        }
        let got = predict_ms(&conn, "foo", ALPHA, Z).unwrap().unwrap();
        assert!(
            got < 2_700_000,
            "after 30 new fast builds the estimate should settle near 2.43 min, \
             got {} ms",
            got
        );
    }

    #[test]
    fn record_finish_with_no_duration_writes_no_row() {
        let conn = open_in_memory().unwrap();
        let mut event = finish("foo", 0, BuildStatus::Cancelled, 999);
        event.duration_ms = None;
        let inserted = record_finish(&conn, &event, 0).unwrap();
        assert!(!inserted);
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM build_observations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn capping_keeps_newest_per_pname() {
        let conn = open_in_memory().unwrap();
        for i in 0..10 {
            record_finish(
                &conn,
                &finish("foo", 100 + i, BuildStatus::Success, 1000 + i),
                3,
            )
            .unwrap();
        }
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM build_observations WHERE pname='foo'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 3);

        // The three newest are durations 107, 108, 109 (matching ts_ms 1007..1009).
        let mut kept: Vec<i64> = Vec::new();
        let mut stmt = conn
            .prepare(
                "SELECT duration_ms FROM build_observations WHERE pname='foo' ORDER BY duration_ms",
            )
            .unwrap();
        for row in stmt.query_map([], |row| row.get::<_, i64>(0)).unwrap() {
            kept.push(row.unwrap());
        }
        assert_eq!(kept, vec![107, 108, 109]);
    }

    #[test]
    fn capping_zero_means_keep_all() {
        let conn = open_in_memory().unwrap();
        for i in 0..5 {
            record_finish(
                &conn,
                &finish("foo", 100 + i, BuildStatus::Success, 1000 + i),
                0,
            )
            .unwrap();
        }
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM build_observations WHERE pname='foo'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 5);
    }

    #[test]
    fn duplicate_finish_inserts_one_row_only() {
        let conn = open_in_memory().unwrap();
        let event = finish("foo", 1000, BuildStatus::Success, 5000);
        assert!(record_finish(&conn, &event, 0).unwrap());
        assert!(!record_finish(&conn, &event, 0).unwrap());
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM build_observations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn out_paths_round_trip_via_newlines() {
        let conn = open_in_memory().unwrap();
        let event = EventBuildFinish {
            drv_path: "/nix/store/abc-foo.drv".to_string(),
            pname: "foo".to_string(),
            host: "tsugumi".to_string(),
            ts_ms: 1000,
            duration_ms: Some(50),
            status: BuildStatus::Success,
            out_paths: vec![
                "/nix/store/out-foo".to_string(),
                "/nix/store/out-foo-doc".to_string(),
            ],
        };
        record_finish(&conn, &event, 0).unwrap();
        let stored: String = conn
            .query_row(
                "SELECT out_paths FROM build_observations LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored, "/nix/store/out-foo\n/nix/store/out-foo-doc");
    }
}
