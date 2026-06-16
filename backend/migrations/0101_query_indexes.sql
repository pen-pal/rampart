-- Indexes matching the actual read patterns of the telemetry tiers. Before
-- these, the logs list, the metric range/anomaly reads, and the RUM per-page
-- query fell back to sequential scans (surfaced by the codebase audit).
--
-- Plain CREATE INDEX (not CONCURRENTLY) — migrations run inside a transaction,
-- and single-tenant Rampart applies them at startup where a brief lock is fine.

-- logs: the filtered list sorts newest-first, often scoped to one service.
CREATE INDEX IF NOT EXISTS logs_ts_idx ON logs (ts DESC);
CREATE INDEX IF NOT EXISTS logs_service_ts_idx ON logs (service_name, ts DESC);
-- logs: the histogram + level-count reads bound on received_at over a window.
CREATE INDEX IF NOT EXISTS logs_received_at_idx ON logs (received_at);

-- metric_samples: the explorer range query + anomaly baseline seek a single
-- series by (name, labels) over a ts window.
CREATE INDEX IF NOT EXISTS metric_samples_series_ts_idx
    ON metric_samples (name, labels, ts DESC);

-- rum_events: the per-page query filters url and sorts newest-first.
CREATE INDEX IF NOT EXISTS rum_events_url_ts_idx ON rum_events (url, ts DESC);
