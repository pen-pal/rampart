-- Multi-DB P1 boot-wiring (SQLite) — threshold/anomaly alert rules over metrics.
--
-- Forked from PG 0073 (metric_rules), 0087 (anomaly op), 0097 (escalation_policy_id),
-- 0108/0112 (org_id NOT NULL). Dialect: uuid→TEXT, jsonb labels→TEXT, UUID[]
-- channel_ids→JSON array TEXT, double→REAL, timestamptz→INTEGER unix-seconds.

CREATE TABLE metric_rules (
  id                   TEXT    PRIMARY KEY,
  name                 TEXT    NOT NULL,
  metric               TEXT    NOT NULL,
  labels               TEXT    NOT NULL DEFAULT '{}',
  op                   TEXT    NOT NULL CHECK (op IN ('gt', 'lt', 'gte', 'lte', 'anomaly')),
  threshold            REAL    NOT NULL,
  for_seconds          INTEGER NOT NULL DEFAULT 0 CHECK (for_seconds >= 0),
  enabled              INTEGER NOT NULL DEFAULT 1,
  channel_ids          TEXT    NOT NULL DEFAULT '[]',
  escalation_policy_id TEXT    REFERENCES escalation_policies(id) ON DELETE SET NULL,
  breach_since         INTEGER,
  firing_at            INTEGER,
  created_at           INTEGER NOT NULL DEFAULT (unixepoch()),
  org_id               TEXT    NOT NULL REFERENCES organizations(id)
);
CREATE INDEX metric_rules_org_idx ON metric_rules (org_id);
