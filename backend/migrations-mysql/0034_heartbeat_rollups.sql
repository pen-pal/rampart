-- Multi-DB P1 domain-port (MySQL) — the heartbeat rollup tier. Forked from the
-- PG/SQLite `heartbeat_rollups` table. The retention prune folds raw heartbeats
-- older than the raw tier into hourly buckets before deleting them, so long-range
-- uptime history survives after the high-resolution rows are gone (retained ~1y
-- by the `rollup_days` tier). Dialect: uuid→CHAR(36), ts→BIGINT unix-seconds
-- (bucket_start truncated to the hour), latency→DOUBLE.

CREATE TABLE heartbeat_rollups (
  monitor_id     CHAR(36) NOT NULL REFERENCES monitors(id) ON DELETE CASCADE,
  bucket_start   BIGINT   NOT NULL,          -- unix seconds truncated to the hour, UTC
  up_count       INT      NOT NULL DEFAULT 0,
  down_count     INT      NOT NULL DEFAULT 0,
  other_count    INT      NOT NULL DEFAULT 0, -- warn/paused/pending/maintenance
  sample_count   INT      NOT NULL DEFAULT 0,
  avg_latency_ms DOUBLE,                       -- NULL when no latency samples
  PRIMARY KEY (monitor_id, bucket_start),
  INDEX heartbeat_rollups_monitor_bucket_idx (monitor_id, bucket_start DESC)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
