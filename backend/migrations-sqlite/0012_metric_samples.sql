-- Multi-DB P1 boot-wiring (SQLite) — external metric samples.
--
-- Forked from PG 0072 (metric_samples) + 0108/0112 (org_id NOT NULL). Dialect:
-- jsonb labels → canonical-JSON TEXT (serde_json default = sorted keys, so TEXT
-- `=` matches PG's semantic jsonb `=`), double→REAL, timestamptz→INTEGER
-- unix-seconds. No PK in PG (append-only series store); same here.

CREATE TABLE metric_samples (
  name   TEXT    NOT NULL,
  labels TEXT    NOT NULL DEFAULT '{}',
  value  REAL    NOT NULL,
  ts     INTEGER NOT NULL DEFAULT (unixepoch()),
  org_id TEXT    NOT NULL REFERENCES organizations(id)
);
CREATE INDEX metric_samples_name_ts_idx ON metric_samples (name, ts DESC);
CREATE INDEX metric_samples_org_idx ON metric_samples (org_id);
