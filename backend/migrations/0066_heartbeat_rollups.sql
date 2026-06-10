-- Heartbeat retention tiering.
--
-- Raw heartbeats are kept for `settings.retention_days.heartbeats` days
-- (the raw tier). Once a heartbeat ages past that window it is rolled up
-- into a coarse hourly aggregate here and the raw row is deleted. These
-- rollups are themselves pruned after the longer rollup tier
-- (`retention_days.rollup_days`, default 365), so historical uptime
-- charts survive far longer than the high-resolution heartbeat stream
-- without unbounded storage growth.

CREATE TABLE heartbeat_rollups (
  monitor_id     UUID             NOT NULL REFERENCES monitors(id) ON DELETE CASCADE,
  bucket_start   TIMESTAMPTZ      NOT NULL,            -- truncated to the hour, UTC
  up_count       INT              NOT NULL DEFAULT 0,
  down_count     INT              NOT NULL DEFAULT 0,
  other_count    INT              NOT NULL DEFAULT 0,  -- warn/paused/pending/maintenance
  sample_count   INT              NOT NULL DEFAULT 0,
  avg_latency_ms DOUBLE PRECISION,                     -- NULL when no latency samples
  PRIMARY KEY (monitor_id, bucket_start)
);

-- Range scans for a monitor over a time window (read helper).
CREATE INDEX heartbeat_rollups_monitor_bucket_idx
  ON heartbeat_rollups (monitor_id, bucket_start DESC);
