-- Multi-DB P1 boot-wiring (SQLite) — service level objectives.
--
-- Forked from PG 0098 (slos) + 0108/0112 (org_id NOT NULL). Dialect: uuid→TEXT,
-- jsonb labels→TEXT, UUID[] channel_ids→JSON, double→REAL, timestamptz→INTEGER
-- unix-seconds. CHECKs (sli_kind, objective_pct range, window_days range) port
-- verbatim.

CREATE TABLE slos (
  id                   TEXT    PRIMARY KEY,
  name                 TEXT    NOT NULL,
  description          TEXT    NOT NULL DEFAULT '',
  sli_kind             TEXT    NOT NULL CHECK (sli_kind IN ('monitor', 'metric')),
  monitor_id           TEXT    REFERENCES monitors(id) ON DELETE CASCADE,
  good_metric          TEXT,
  total_metric         TEXT,
  labels               TEXT    NOT NULL DEFAULT '{}',
  objective_pct        REAL    NOT NULL CHECK (objective_pct > 0 AND objective_pct < 100),
  window_days          INTEGER NOT NULL DEFAULT 30 CHECK (window_days BETWEEN 1 AND 365),
  enabled              INTEGER NOT NULL DEFAULT 1,
  channel_ids          TEXT    NOT NULL DEFAULT '[]',
  escalation_policy_id TEXT    REFERENCES escalation_policies(id) ON DELETE SET NULL,
  breaching_at         INTEGER,
  created_at           INTEGER NOT NULL DEFAULT (unixepoch()),
  org_id               TEXT    NOT NULL REFERENCES organizations(id)
);
CREATE INDEX slos_org_idx ON slos (org_id);
