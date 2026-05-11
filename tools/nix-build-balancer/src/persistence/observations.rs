use std::io;

use rusqlite::{params, Connection};

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

/// p95 of successful build durations for `pname`. None when no rows match.
pub fn p95_ms(conn: &Connection, pname: &str) -> io::Result<Option<u64>> {
    let mut stmt = conn
        .prepare(
            "SELECT duration_ms FROM build_observations
             WHERE status = 'success' AND pname = ?1
             ORDER BY duration_ms",
        )
        .map_err(io::Error::other)?;
    let rows = stmt
        .query_map(params![pname], |row| row.get::<_, i64>(0))
        .map_err(io::Error::other)?;
    let mut values = Vec::new();
    for row in rows {
        let duration = row.map_err(io::Error::other)?;
        if duration >= 0 {
            values.push(duration as u64);
        }
    }
    Ok(quantile(&values, 0.95))
}

/// Upper-bucket quantile for already-sorted duration values.
/// Returns `None` for an empty slice.
pub fn quantile(values: &[u64], q: f64) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let idx = ((values.len() - 1) as f64 * q).ceil() as usize;
    values.get(idx).copied()
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

    #[test]
    fn quantile_upper_bucket() {
        let values = [10, 20, 30, 40];
        assert_eq!(quantile(&values, 0.50), Some(30));
        assert_eq!(quantile(&values, 0.95), Some(40));
    }

    #[test]
    fn quantile_empty_is_none() {
        assert_eq!(quantile(&[], 0.95), None);
    }

    #[test]
    fn p95_none_on_empty_history() {
        let conn = open_in_memory().unwrap();
        assert_eq!(p95_ms(&conn, "foo").unwrap(), None);
    }

    #[test]
    fn p95_returns_upper_bucket() {
        let conn = open_in_memory().unwrap();
        for (i, d) in [10u64, 20, 30, 40].iter().enumerate() {
            record_finish(
                &conn,
                &finish("foo", *d, BuildStatus::Success, 1000 + i as u64),
                0,
            )
            .unwrap();
        }
        assert_eq!(p95_ms(&conn, "foo").unwrap(), Some(40));
    }

    #[test]
    fn p95_ignores_failure_rows() {
        let conn = open_in_memory().unwrap();
        record_finish(&conn, &finish("foo", 5, BuildStatus::Success, 100), 0).unwrap();
        record_finish(&conn, &finish("foo", 999, BuildStatus::Failure, 200), 0).unwrap();
        // Failure row excluded; p95 of [5] is 5.
        assert_eq!(p95_ms(&conn, "foo").unwrap(), Some(5));
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
