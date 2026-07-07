-- Multi-DB P1 domain-port (SQLite) — the heartbeat rollup tier. Forked from the
-- PG `heartbeat_rollups` table. The retention prune folds raw heartbeats older
-- than the raw tier into hourly buckets before deleting them, so long-range
-- uptime history survives after the high-resolution rows are gone (retained
-- ~1y by the `rollup_days` tier). Dialect: uuid→TEXT, ts→INTEGER unix-seconds
-- (bucket_start truncated to the hour), latency→REAL.

CREATE TABLE heartbeat_rollups (
  monitor_id     TEXT    NOT NULL REFERENCES monitors(id) ON DELETE CASCADE,
  bucket_start   INTEGER NOT NULL,           -- unix seconds truncated to the hour, UTC
  up_count       INTEGER NOT NULL DEFAULT 0,
  down_count     INTEGER NOT NULL DEFAULT 0,
  other_count    INTEGER NOT NULL DEFAULT 0, -- warn/paused/pending/maintenance
  sample_count   INTEGER NOT NULL DEFAULT 0,
  avg_latency_ms REAL,                        -- NULL when no latency samples
  PRIMARY KEY (monitor_id, bucket_start)
);
CREATE INDEX heartbeat_rollups_monitor_bucket_idx
  ON heartbeat_rollups (monitor_id, bucket_start DESC);
