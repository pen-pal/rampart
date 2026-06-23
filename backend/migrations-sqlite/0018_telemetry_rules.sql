-- Multi-DB P1 boot-wiring (SQLite) — telemetry alert rules.
--
-- Forked from PG 0081 (telemetry_alert_rules) + escalation FK + 0108/0112 (org).
-- Dialect: uuid→TEXT, smallint/int→INTEGER, double→REAL, UUID[] channel_ids→
-- JSON, ts→INTEGER unix-seconds. The `kind` CHECK is omitted (app-validated via
-- the TelemetryRuleKind serde round-trip — same precedent as monitors.kind);
-- the `op` CHECK ports verbatim.

CREATE TABLE telemetry_alert_rules (
  id                   TEXT    PRIMARY KEY,
  name                 TEXT    NOT NULL,
  kind                 TEXT    NOT NULL,
  target               TEXT    NOT NULL DEFAULT '',
  match_text           TEXT    NOT NULL DEFAULT '',
  min_level            INTEGER NOT NULL DEFAULT 0,
  op                   TEXT    NOT NULL CHECK (op IN ('gt', 'lt', 'gte', 'lte')),
  threshold            REAL    NOT NULL,
  window_seconds       INTEGER NOT NULL DEFAULT 300 CHECK (window_seconds > 0),
  for_seconds          INTEGER NOT NULL DEFAULT 0 CHECK (for_seconds >= 0),
  enabled              INTEGER NOT NULL DEFAULT 1,
  channel_ids          TEXT    NOT NULL DEFAULT '[]',
  escalation_policy_id TEXT    REFERENCES escalation_policies(id) ON DELETE SET NULL,
  breach_since         INTEGER,
  firing_at            INTEGER,
  created_at           INTEGER NOT NULL DEFAULT (unixepoch()),
  org_id               TEXT    NOT NULL REFERENCES organizations(id)
);
CREATE INDEX telemetry_alert_rules_org_idx ON telemetry_alert_rules (org_id);
