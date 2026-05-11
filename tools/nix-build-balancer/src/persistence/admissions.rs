use std::io;

use rusqlite::{params, Connection};

/// One row of the `admissions` table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdmissionRow {
    pub drv_path: String,
    pub target_name: String,
    pub admitted_at_ms: u64,
    pub predicted_ms: u64,
}

/// Insert or update an admission. Spec §"Build observation lifecycle" item
/// 1: at most one row per `drv_path`; re-admission overwrites.
pub fn record(
    conn: &Connection,
    drv_path: &str,
    target_name: &str,
    admitted_at_ms: u64,
    predicted_ms: u64,
) -> io::Result<()> {
    conn.execute(
        "INSERT INTO admissions (drv_path, target_name, admitted_at_ms, predicted_ms)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(drv_path) DO UPDATE SET
           target_name    = excluded.target_name,
           admitted_at_ms = excluded.admitted_at_ms,
           predicted_ms   = excluded.predicted_ms",
        params![
            drv_path,
            target_name,
            admitted_at_ms as i64,
            predicted_ms as i64,
        ],
    )
    .map_err(io::Error::other)?;
    Ok(())
}

/// Delete the admission row for `drv_path`. Returns whether a row was
/// actually removed (callers ignore the boolean on retirement paths, but
/// duplicate `EVENT_BUILD_FINISH` arrivals see `false` on the second call,
/// which is exactly the no-op the spec wants).
pub fn retire(conn: &Connection, drv_path: &str) -> io::Result<bool> {
    let changed = conn
        .execute(
            "DELETE FROM admissions WHERE drv_path = ?1",
            params![drv_path],
        )
        .map_err(io::Error::other)?;
    Ok(changed > 0)
}

pub fn list(conn: &Connection) -> io::Result<Vec<AdmissionRow>> {
    let mut stmt = conn
        .prepare(
            "SELECT drv_path, target_name, admitted_at_ms, predicted_ms
             FROM admissions
             ORDER BY admitted_at_ms",
        )
        .map_err(io::Error::other)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(AdmissionRow {
                drv_path: row.get(0)?,
                target_name: row.get(1)?,
                admitted_at_ms: row.get::<_, i64>(2)?.max(0) as u64,
                predicted_ms: row.get::<_, i64>(3)?.max(0) as u64,
            })
        })
        .map_err(io::Error::other)?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(io::Error::other)?);
    }
    Ok(result)
}

/// Drv paths whose admissions have outlived `max(predicted_ms * 2, 60_000)`.
/// Watchdog reads this on every tick and synthesises an [`AdmissionFinish`]
/// for each stale row before retiring it.
pub fn stale_drvs(conn: &Connection, now_ms: u64) -> io::Result<Vec<String>> {
    Ok(list(conn)?
        .into_iter()
        .filter_map(|row| {
            let ttl = row.predicted_ms.saturating_mul(2).max(60_000);
            let age = now_ms.saturating_sub(row.admitted_at_ms);
            (age > ttl).then_some(row.drv_path)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::open_in_memory;

    #[test]
    fn record_and_list_in_admission_order() {
        let conn = open_in_memory().unwrap();
        record(&conn, "/nix/store/b-bar.drv", "tsugumi", 200, 7_000).unwrap();
        record(&conn, "/nix/store/a-foo.drv", "tsugumi", 100, 5_000).unwrap();
        let rows = list(&conn).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].drv_path, "/nix/store/a-foo.drv");
        assert_eq!(rows[1].drv_path, "/nix/store/b-bar.drv");
        assert_eq!(rows[0].predicted_ms, 5_000);
        assert_eq!(rows[1].target_name, "tsugumi");
    }

    #[test]
    fn re_admission_overwrites() {
        let conn = open_in_memory().unwrap();
        record(&conn, "/nix/store/a-foo.drv", "tsugumi", 100, 5_000).unwrap();
        record(&conn, "/nix/store/a-foo.drv", "saya", 300, 9_000).unwrap();
        let rows = list(&conn).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].target_name, "saya");
        assert_eq!(rows[0].admitted_at_ms, 300);
        assert_eq!(rows[0].predicted_ms, 9_000);
    }

    #[test]
    fn retire_returns_true_on_first_then_false() {
        let conn = open_in_memory().unwrap();
        record(&conn, "/nix/store/a-foo.drv", "tsugumi", 100, 5_000).unwrap();
        assert!(retire(&conn, "/nix/store/a-foo.drv").unwrap());
        assert!(!retire(&conn, "/nix/store/a-foo.drv").unwrap());
        assert!(list(&conn).unwrap().is_empty());
    }

    #[test]
    fn stale_drvs_uses_max_of_predicted_times_two_and_sixty_seconds() {
        let conn = open_in_memory().unwrap();
        // Predicted 10s, admitted at t=0. TTL = max(20s, 60s) = 60s.
        record(&conn, "/nix/store/short.drv", "tsugumi", 0, 10_000).unwrap();
        // Predicted 90s, admitted at t=0. TTL = max(180s, 60s) = 180s.
        record(&conn, "/nix/store/long.drv", "tsugumi", 0, 90_000).unwrap();

        // At t=30s, nothing stale yet.
        assert!(stale_drvs(&conn, 30_000).unwrap().is_empty());

        // At t=70s, only the short admission has exceeded its 60s long-stop.
        let stale = stale_drvs(&conn, 70_000).unwrap();
        assert_eq!(stale, vec!["/nix/store/short.drv".to_string()]);

        // At t=200s, both are stale.
        let mut stale = stale_drvs(&conn, 200_000).unwrap();
        stale.sort();
        assert_eq!(
            stale,
            vec![
                "/nix/store/long.drv".to_string(),
                "/nix/store/short.drv".to_string()
            ]
        );
    }

    #[test]
    fn stale_drvs_handles_zero_predicted_ms_with_sixty_second_floor() {
        let conn = open_in_memory().unwrap();
        record(&conn, "/nix/store/zero.drv", "tsugumi", 0, 0).unwrap();
        // TTL = max(0, 60_000) = 60_000.
        assert!(stale_drvs(&conn, 30_000).unwrap().is_empty());
        assert_eq!(
            stale_drvs(&conn, 70_000).unwrap(),
            vec!["/nix/store/zero.drv".to_string()]
        );
    }
}
