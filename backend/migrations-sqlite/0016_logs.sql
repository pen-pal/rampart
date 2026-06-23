-- Multi-DB P1 boot-wiring (SQLite) — log storage (telemetry foundation).
--
-- Forked from PG 0079 (logs) + 0108/0112 (org_id NOT NULL). The PG 0082
-- `body_tsv` generated tsvector + GIN full-text index has NO SQLite equivalent;
-- the SQLite query layer degrades body search to a `LIKE` substring match
-- (no phrase/OR/negation) — acceptable for the single-binary homelab tier.
-- Dialect: uuid→TEXT, smallint→INTEGER, jsonb attributes→TEXT, timestamptz→
-- INTEGER unix-seconds. `received_at` defaults to insert time, like PG.

CREATE TABLE logs (
  id            TEXT    PRIMARY KEY,
  ts            INTEGER NOT NULL,
  severity      INTEGER NOT NULL DEFAULT 0,
  severity_text TEXT,
  service_name  TEXT    NOT NULL DEFAULT 'unknown',
  body          TEXT    NOT NULL DEFAULT '',
  trace_id      TEXT,
  span_id       TEXT,
  attributes    TEXT,
  received_at   INTEGER NOT NULL DEFAULT (unixepoch()),
  org_id        TEXT    NOT NULL REFERENCES organizations(id)
);
CREATE INDEX logs_recent_idx  ON logs (received_at DESC);
CREATE INDEX logs_service_idx ON logs (service_name, received_at DESC);
CREATE INDEX logs_org_idx     ON logs (org_id);
CREATE INDEX logs_trace_idx   ON logs (trace_id);
