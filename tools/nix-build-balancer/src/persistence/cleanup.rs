use rusqlite::{params, Connection};
use std::io;

use crate::scheduler::policy::SchedulerPolicy;
use crate::util::{now_ms, sqlite_error, timestamp_to_i64};

const MIN_STALE_ADMISSION_MS: i64 = 60_000;

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

/// Clear volatile in-flight state after the daemon starts.
///
/// A daemon restart normally means the previous Nix build session ended through
/// a system update, service restart, or reboot. Completed build observations are
/// durable history, but unmatched starts and remote admissions are only live
/// accounting and should not survive that boundary.
pub fn clear_ongoing_builds(conn: &Connection) -> io::Result<()> {
    let active = conn
        .execute("DELETE FROM active_builds", [])
        .map_err(sqlite_error)?;
    let admissions = conn
        .execute("DELETE FROM remote_admissions", [])
        .map_err(sqlite_error)?;
    if active > 0 || admissions > 0 {
        tracing::info!(
            active_builds_removed = active,
            remote_admissions_removed = admissions,
            "ongoing_builds_cleared",
        );
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
    let now = timestamp_to_i64(now_ms())?;
    let fallback_ttl = duration_to_stale_admission_ttl(policy.unknown_p95_ms);
    conn.execute(
        "DELETE FROM remote_admissions
         WHERE admitted_at_ms + max(predicted_ms * 2, ?1) < ?2
            OR admitted_at_ms + ?3 < ?2",
        params![MIN_STALE_ADMISSION_MS, now, fallback_ttl],
    )
    .map_err(sqlite_error)?;
    Ok(())
}

fn duration_to_stale_admission_ttl(duration_ms: u64) -> i64 {
    i64::try_from(duration_ms)
        .unwrap_or(i64::MAX / 2)
        .saturating_mul(2)
        .max(MIN_STALE_ADMISSION_MS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::events::record_event;
    use crate::persistence::open_history_db;
    use crate::scheduler::policy::SchedulerPolicy;
    use crate::test_support::{
        build_event, insert_remote_admission, remote_admission_count, test_config, test_data_dir,
    };
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

    #[test]
    fn stale_remote_admissions_expire_by_prediction() {
        let dir = test_data_dir("stale-remote-admission");
        let conn = open_history_db(&dir).unwrap();
        insert_remote_admission(
            &conn,
            "/nix/store/hash-short-remote-build.drv",
            now_ms().saturating_sub(MIN_STALE_ADMISSION_MS as u128 + 1_000),
            1_000,
            false,
        );

        cleanup_stale_admissions_with_policy(&conn, &SchedulerPolicy::default()).unwrap();

        assert_eq!(remote_admission_count(&dir), 0);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn fresh_remote_admissions_are_retained() {
        let dir = test_data_dir("fresh-remote-admission");
        let conn = open_history_db(&dir).unwrap();
        insert_remote_admission(
            &conn,
            "/nix/store/hash-fresh-remote-build.drv",
            now_ms().saturating_sub(MIN_STALE_ADMISSION_MS as u128 / 2),
            1_000,
            false,
        );

        cleanup_stale_admissions_with_policy(&conn, &SchedulerPolicy::default()).unwrap();

        assert_eq!(remote_admission_count(&dir), 1);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn startup_cleanup_clears_volatile_build_state_only() {
        let dir = test_data_dir("startup-cleanup");
        let cfg = test_config(dir.clone());
        let conn = open_history_db(&dir).unwrap();
        let active_drv = "/nix/store/hash-active-build.drv";
        let completed_drv = "/nix/store/hash-completed-build.drv";

        record_event(&cfg, &build_event("start", active_drv, now_ms(), "unknown")).unwrap();
        record_event(
            &cfg,
            &build_event(
                "start",
                completed_drv,
                now_ms().saturating_sub(2_000),
                "unknown",
            ),
        )
        .unwrap();
        record_event(
            &cfg,
            &build_event(
                "finish",
                completed_drv,
                now_ms().saturating_sub(1_000),
                "success",
            ),
        )
        .unwrap();
        insert_remote_admission(
            &conn,
            "/nix/store/hash-remote-build.drv",
            now_ms(),
            10_000,
            false,
        );

        clear_ongoing_builds(&conn).unwrap();

        let active_count: i64 = conn
            .query_row("SELECT count(*) FROM active_builds", [], |row| row.get(0))
            .unwrap();
        let observation_count: i64 = conn
            .query_row("SELECT count(*) FROM build_observations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(active_count, 0);
        assert_eq!(remote_admission_count(&dir), 0);
        assert_eq!(observation_count, 1);
        let _ = fs::remove_dir_all(dir);
    }
}
