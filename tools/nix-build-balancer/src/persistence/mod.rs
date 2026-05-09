pub mod cleanup;
pub mod events;
pub mod queries;

use rusqlite::{params, Connection};
use std::fs;
use std::io;
use std::path::Path;
use std::time::Duration;

use crate::config::Config;
use crate::util::sqlite_error;

const SCHEMA_VERSION: i64 = 1;

pub use cleanup::cleanup_stale_starts;
pub use events::{finish_admission, record_event};
pub use queries::stats_json;

/// Open the history database and ensure the current schema exists.
pub fn open_history_db(data_dir: &Path) -> io::Result<Connection> {
    fs::create_dir_all(data_dir)?;
    let conn = Connection::open(data_dir.join("history.sqlite3")).map_err(sqlite_error)?;
    conn.busy_timeout(Duration::from_secs(5))
        .map_err(sqlite_error)?;
    init_history_schema(&conn)?;
    Ok(conn)
}

/// Create the SQLite schema used by build observation and admission tracking.
pub fn init_history_schema(conn: &Connection) -> io::Result<()> {
    conn.execute_batch(
        "
        PRAGMA journal_mode = WAL;
        PRAGMA foreign_keys = ON;
        CREATE TABLE IF NOT EXISTS meta (
          key TEXT PRIMARY KEY,
          value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS active_builds (
          drv_path TEXT PRIMARY KEY,
          host TEXT NOT NULL,
          pname TEXT NOT NULL,
          started_at_ms INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS build_observations (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          host TEXT NOT NULL,
          pname TEXT NOT NULL,
          drv_path TEXT NOT NULL,
          started_at_ms INTEGER NOT NULL,
          finished_at_ms INTEGER NOT NULL,
          duration_ms INTEGER NOT NULL,
          status TEXT NOT NULL,
          out_paths TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_build_observations_pname_finished
          ON build_observations (pname, finished_at_ms DESC, id DESC);
        CREATE INDEX IF NOT EXISTS idx_build_observations_success_pname
          ON build_observations (pname, duration_ms)
          WHERE status = 'success';
        CREATE TABLE IF NOT EXISTS remote_admissions (
          drv_path TEXT PRIMARY KEY,
          host TEXT NOT NULL,
          pname TEXT NOT NULL,
          admitted_at_ms INTEGER NOT NULL,
          predicted_ms INTEGER NOT NULL,
          unknown INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_remote_admissions_host
          ON remote_admissions (host, admitted_at_ms);
        ",
    )
    .map_err(sqlite_error)?;
    conn.execute(
        "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![SCHEMA_VERSION.to_string()],
    )
    .map_err(sqlite_error)?;
    Ok(())
}

pub fn cleanup_state(cfg: &Config) -> io::Result<()> {
    let conn = open_history_db(&cfg.data_dir)?;
    cleanup_stale_starts(&conn, cfg.stale_start_ms)
}
