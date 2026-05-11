pub mod admissions;
pub mod observations;

use rusqlite::Connection;
use std::io;
use std::path::Path;

const SCHEMA: &str = include_str!("schema.sql");

pub fn open<P: AsRef<Path>>(path: P) -> io::Result<Connection> {
    let conn = Connection::open(path).map_err(io::Error::other)?;
    init_schema(&conn)?;
    Ok(conn)
}

/// Open an in-memory SQLite database. Used by tests and by lifecycle tests.
pub fn open_in_memory() -> io::Result<Connection> {
    let conn = Connection::open_in_memory().map_err(io::Error::other)?;
    init_schema(&conn)?;
    Ok(conn)
}

pub fn init_schema(conn: &Connection) -> io::Result<()> {
    conn.execute_batch(SCHEMA).map_err(io::Error::other)
}

/// Drop every admissions row. The controller calls this on startup
/// (spec invariant 6: a restart loses in-flight knowledge).
pub fn clear_admissions(conn: &Connection) -> io::Result<()> {
    conn.execute("DELETE FROM admissions", [])
        .map_err(io::Error::other)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_creates_expected_tables() {
        let conn = open_in_memory().unwrap();
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap();
        let names: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert!(names.contains(&"build_observations".to_string()));
        assert!(names.contains(&"admissions".to_string()));
        assert!(names.contains(&"meta".to_string()));
    }

    #[test]
    fn schema_version_is_one() {
        let conn = open_in_memory().unwrap();
        let version: String = conn
            .query_row(
                "SELECT value FROM meta WHERE key='schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, "1");
    }

    #[test]
    fn clear_admissions_empties_table() {
        let conn = open_in_memory().unwrap();
        admissions::record(&conn, "/nix/store/a-foo.drv", "tsugumi", 100, 5000).unwrap();
        admissions::record(&conn, "/nix/store/b-bar.drv", "tsugumi", 200, 7000).unwrap();
        assert_eq!(admissions::list(&conn).unwrap().len(), 2);
        clear_admissions(&conn).unwrap();
        assert!(admissions::list(&conn).unwrap().is_empty());
    }

    #[test]
    fn idempotent_init_schema() {
        let conn = open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        init_schema(&conn).unwrap();
        admissions::record(&conn, "/nix/store/a-foo.drv", "tsugumi", 100, 5000).unwrap();
        assert_eq!(admissions::list(&conn).unwrap().len(), 1);
    }
}
