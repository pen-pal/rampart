-- External metric ingestion (observability tier, batch-18).
--
-- Flat sample store for metrics pushed from outside — cron jobs, scripts,
-- the rampart-agent — via POST /v1/metrics/ingest (Prometheus text
-- exposition format, Pushgateway-style). One row per sample; `labels` is
-- the full label set as JSONB so arbitrary dimensions need no schema.
-- Timestamps are server-stamped at ingest (client timestamps in the text
-- format are ignored — remote clocks are never trusted, same stance as
-- push/agent heartbeats).
--
-- Deliberately not a TSDB: Postgres comfortably holds homelab-scale
-- cardinality (thousands of series, weeks of retention — the prune loop
-- trims by age like heartbeats). Threshold alert rules (migration 0073)
-- evaluate against the latest sample per series.
CREATE TABLE metric_samples (
    name   TEXT             NOT NULL,
    labels JSONB            NOT NULL DEFAULT '{}',
    value  DOUBLE PRECISION NOT NULL,
    ts     TIMESTAMPTZ      NOT NULL DEFAULT now()
);

-- Range queries and latest-sample lookups are always name-scoped,
-- newest-first.
CREATE INDEX metric_samples_name_ts_idx ON metric_samples (name, ts DESC);
