-- Multi-DB P1 boot-wiring (SQLite) — SIEM detection rules + findings.
--
-- Forked from PG 0090 (rules+findings) + 0091 (attr match) + 0103 (cooldown) +
-- 0104 (condition tree) + 0105 (group_by/entity) + 0108/0112 (org_id). The eval
-- layer matches over the ported `logs` tier. Dialect: uuid→TEXT, bool→INTEGER
-- 0/1, smallint/int→INTEGER, ts→INTEGER unix-seconds, UUID[] channel_ids→JSON,
-- jsonb condition→TEXT(JSON). The PG `body ~* regex` is degraded to a
-- case-insensitive LIKE substring in the query layer (no SQLite regex) — same
-- single-binary homelab compromise as the logs FTS degrade.

CREATE TABLE detection_rules (
  id              TEXT    PRIMARY KEY,
  name            TEXT    NOT NULL,
  description     TEXT    NOT NULL DEFAULT '',
  enabled         INTEGER NOT NULL DEFAULT 1,
  severity        TEXT    NOT NULL DEFAULT 'medium'
                          CHECK (severity IN ('low', 'medium', 'high', 'critical')),
  service         TEXT    NOT NULL DEFAULT '',
  min_level       INTEGER NOT NULL DEFAULT 0,
  body_regex      TEXT    NOT NULL DEFAULT '',
  attr_key        TEXT    NOT NULL DEFAULT '',
  attr_val        TEXT    NOT NULL DEFAULT '',
  threshold       INTEGER NOT NULL DEFAULT 1,
  window_seconds  INTEGER NOT NULL DEFAULT 300,
  cooldown_seconds INTEGER NOT NULL DEFAULT 0,
  group_by        TEXT    NOT NULL DEFAULT '',
  condition       TEXT,
  channel_ids     TEXT    NOT NULL DEFAULT '[]',
  escalation_policy_id TEXT REFERENCES escalation_policies(id) ON DELETE SET NULL,
  last_checked_at INTEGER,
  created_at      INTEGER NOT NULL DEFAULT (unixepoch()),
  org_id          TEXT    NOT NULL REFERENCES organizations(id)
);
CREATE INDEX detection_rules_org_idx ON detection_rules (org_id);

CREATE TABLE detection_findings (
  id              TEXT    PRIMARY KEY,
  rule_id         TEXT    NOT NULL REFERENCES detection_rules(id) ON DELETE CASCADE,
  rule_name       TEXT    NOT NULL,
  severity        TEXT    NOT NULL
                          CHECK (severity IN ('low', 'medium', 'high', 'critical')),
  match_count     INTEGER NOT NULL,
  sample          TEXT,
  service         TEXT,
  entity          TEXT,
  window_from     INTEGER NOT NULL,
  window_to       INTEGER NOT NULL,
  created_at      INTEGER NOT NULL DEFAULT (unixepoch()),
  acknowledged_at INTEGER
);
CREATE INDEX detection_findings_recent_idx ON detection_findings (created_at DESC);
CREATE INDEX detection_findings_open_idx ON detection_findings (created_at DESC)
  WHERE acknowledged_at IS NULL;
CREATE INDEX detection_findings_rule_idx ON detection_findings (rule_id, created_at DESC);
