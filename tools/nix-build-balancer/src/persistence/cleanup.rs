use rusqlite::{params, Connection};
use std::io;

use crate::scheduler::policy::SchedulerPolicy;
use crate::util::{now_ms, sqlite_error, timestamp_to_i64};

/// Remove pre-build starts that never received a matching post-build event.
pub fn cleanup_stale_starts(conn: &Connection, stale_start_ms: u128) -> io::Result<()> {
    if stale_start_ms == 0 {
        return Ok(());
    }
    let cutoff = timestamp_to_i64(now_ms().saturating_sub(stale_start_ms))?;
    let removed = conn
        .execute(
            "DELETE FROM active_builds WHERE started_at_ms < ?1",
            params![cutoff],
        )
        .map_err(sqlite_error)?;
    if removed > 0 {
        eprintln!("stale_starts_removed count={removed}");
    }
    Ok(())
}

/// Keep only the newest successful and failed observations for one package.
pub fn prune_pname_samples(
    conn: &Connection,
    pname: &str,
    max_samples_per_pname: usize,
) -> io::Result<()> {
    if max_samples_per_pname == 0 {
        return Ok(());
    }
    conn.execute(
        "DELETE FROM build_observations
         WHERE pname = ?1
           AND id NOT IN (
             SELECT id FROM build_observations
             WHERE pname = ?1
             ORDER BY finished_at_ms DESC, id DESC
             LIMIT ?2
           )",
        params![pname, max_samples_per_pname as i64],
    )
    .map_err(sqlite_error)?;
    Ok(())
}

/// Remove admissions old enough that the delegated build likely disappeared.
pub fn cleanup_stale_admissions_with_policy(
    conn: &Connection,
    policy: &SchedulerPolicy,
) -> io::Result<()> {
    let cutoff = timestamp_to_i64(now_ms().saturating_sub(policy.unknown_p95_ms as u128 * 2))?;
    conn.execute(
        "DELETE FROM remote_admissions WHERE admitted_at_ms < ?1",
        params![cutoff],
    )
    .map_err(sqlite_error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::persistence::events::record_event;
    use crate::persistence::open_history_db;
    use crate::test_support::{build_event, test_config, test_data_dir};
    use std::fs;

    #[test]
    fn retention_is_per_pname() {
        let dir = test_data_dir("retention");
        let mut cfg = test_config(dir.clone());
        cfg.max_samples_per_pname = 2;

        for i in 0..3 {
            let drv = format!("/nix/store/hash-kwin-6.6.{i}.drv");
            record_event(
                &cfg,
                &build_event("start", &drv, 1_000 + i * 1_000, "unknown"),
            )
            .unwrap();
            record_event(
                &cfg,
                &build_event("finish", &drv, 1_500 + i * 1_000, "success"),
            )
            .unwrap();
        }

        let other_drv = "/nix/store/hash-linux-6.19.5.drv";
        record_event(&cfg, &build_event("start", other_drv, 10_000, "unknown")).unwrap();
        record_event(&cfg, &build_event("finish", other_drv, 11_000, "success")).unwrap();

        let conn = open_history_db(&dir).unwrap();
        let kwin_count: i64 = conn
            .query_row(
                "SELECT count(*) FROM build_observations WHERE pname = 'kwin'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let linux_count: i64 = conn
            .query_row(
                "SELECT count(*) FROM build_observations WHERE pname = 'linux'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(kwin_count, 2);
        assert_eq!(linux_count, 1);
        let _ = fs::remove_dir_all(dir);
    }
}
