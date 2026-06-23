-- Multi-DB P1 boot-wiring (SQLite) — trace spans (telemetry foundation 2/2).
--
-- Forked from PG 0078 (spans) + 0108/0112 (org_id NOT NULL). Dialect: hex ids
-- stay TEXT; smallint→INTEGER; ns→INTEGER (i64); double→REAL; jsonb attrs→TEXT;
-- received_at→INTEGER unix-seconds. span_id PRIMARY KEY backs the insert
-- ON CONFLICT DO NOTHING (exporters retransmit).

CREATE TABLE spans (
  span_id        TEXT    PRIMARY KEY,
  trace_id       TEXT    NOT NULL,
  parent_span_id TEXT,
  service_name   TEXT    NOT NULL DEFAULT 'unknown',
  name           TEXT    NOT NULL,
  kind           INTEGER NOT NULL DEFAULT 0,
  start_ns       INTEGER NOT NULL,
  end_ns         INTEGER NOT NULL,
  duration_ms    REAL    NOT NULL,
  status_code    INTEGER NOT NULL DEFAULT 0,
  status_message TEXT,
  attributes     TEXT,
  received_at    INTEGER NOT NULL DEFAULT (unixepoch()),
  org_id         TEXT    NOT NULL REFERENCES organizations(id)
);
CREATE INDEX spans_recent_idx  ON spans (received_at DESC);
CREATE INDEX spans_service_idx ON spans (service_name, received_at DESC);
CREATE INDEX spans_trace_idx   ON spans (trace_id);
CREATE INDEX spans_org_idx     ON spans (org_id);
