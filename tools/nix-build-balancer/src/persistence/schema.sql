CREATE TABLE IF NOT EXISTS build_observations (
  host           TEXT    NOT NULL,
  pname          TEXT    NOT NULL,
  drv_path       TEXT    NOT NULL,
  started_at_ms  INTEGER NOT NULL,
  finished_at_ms INTEGER NOT NULL,
  duration_ms    INTEGER NOT NULL,
  status         TEXT    NOT NULL,
  out_paths      TEXT    NOT NULL
);

CREATE INDEX IF NOT EXISTS build_observations_pname
  ON build_observations(pname);

CREATE UNIQUE INDEX IF NOT EXISTS build_observations_drv_ts
  ON build_observations(drv_path, finished_at_ms);

CREATE TABLE IF NOT EXISTS admissions (
  drv_path       TEXT    PRIMARY KEY,
  target_name    TEXT    NOT NULL,
  admitted_at_ms INTEGER NOT NULL,
  predicted_ms   INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

INSERT OR IGNORE INTO meta(key, value) VALUES ('schema_version', '1');
