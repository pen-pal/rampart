-- ─── Multi-tenancy Phase 5: composite org_id indexes for the telemetry tiers ──
--
-- Phase 5 stamps org_id at ingest (P5-1..4) and scopes every telemetry read /
-- aggregate to `WHERE org_id = $` (P5-7..9). Those filters now sit in front of
-- the time-ordered scans the tiers were tuned for in 0101, so add composite
-- (org_id, <time>) indexes that let a single-org query stay on the index and a
-- multi-org query seek straight to its tenant's slice.
--
-- Plain CREATE INDEX (not CONCURRENTLY) — migrations run inside a transaction,
-- and Rampart applies them at startup where a brief lock is fine.

-- logs: filtered list sorts newest-first on (ts) within an org.
CREATE INDEX IF NOT EXISTS logs_org_ts_idx ON logs (org_id, ts DESC);

-- spans: list / service-map / operation reads bound on received_at within an org.
CREATE INDEX IF NOT EXISTS spans_org_received_at_idx ON spans (org_id, received_at DESC);

-- metric_samples: series list / range / baseline seek a series over a ts window
-- within an org.
CREATE INDEX IF NOT EXISTS metric_samples_org_ts_idx ON metric_samples (org_id, ts DESC);

-- rum_events: per-page / summary / breakdown reads bound on received_at within
-- an org.
CREATE INDEX IF NOT EXISTS rum_events_org_received_at_idx ON rum_events (org_id, received_at DESC);

-- profiles: list / flamegraph-window reads bound on received_at within an org.
CREATE INDEX IF NOT EXISTS profiles_org_received_at_idx ON profiles (org_id, received_at DESC);
