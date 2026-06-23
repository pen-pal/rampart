-- Monitors + heartbeats (core monitoring entity), forked from the Postgres
-- schema. SQLite mapping:
--   uuid              -> TEXT
--   monitor_kind enum -> TEXT (40+ kinds; app-validated, no CHECK)
--   monitor_status    -> TEXT
--   jsonb (config/http_headers) -> TEXT
--   int[] accepted_statuses     -> TEXT (JSON array)
--   numeric slo_target_pct      -> REAL
--   timestamptz       -> INTEGER unix-seconds
--   boolean           -> INTEGER 0/1
-- proxy_id/group_id/agent_id/escalation_policy_id are plain TEXT (no FK yet —
-- those domains aren't on SQLite; org_id FKs organizations).
CREATE TABLE monitors (
  id                  TEXT    PRIMARY KEY,
  name                TEXT    NOT NULL,
  kind                TEXT    NOT NULL,
  url                 TEXT,
  hostname            TEXT,
  port                INTEGER,
  config              TEXT    NOT NULL DEFAULT '{}',
  interval_seconds    INTEGER NOT NULL,
  retry_interval_sec  INTEGER NOT NULL DEFAULT 60,
  max_retries         INTEGER NOT NULL DEFAULT 0,
  timeout_seconds     INTEGER NOT NULL,
  resend_interval_sec INTEGER NOT NULL DEFAULT 0,
  upside_down         INTEGER NOT NULL DEFAULT 0,
  http_method         TEXT    NOT NULL DEFAULT 'GET',
  http_body           TEXT,
  http_headers        TEXT,
  accepted_statuses   TEXT    NOT NULL DEFAULT '[200]',
  follow_redirect     INTEGER NOT NULL DEFAULT 1,
  ignore_tls          INTEGER NOT NULL DEFAULT 0,
  proxy_id            TEXT,
  active              INTEGER NOT NULL DEFAULT 1,
  current_status      TEXT    NOT NULL DEFAULT 'pending',
  created_at          INTEGER NOT NULL DEFAULT (unixepoch()),
  updated_at          INTEGER NOT NULL DEFAULT (unixepoch()),
  push_token          TEXT,
  last_push_at        INTEGER,
  cert_days_left      INTEGER,
  cert_subject        TEXT,
  cert_checked_at     INTEGER,
  group_id            TEXT,
  slo_target_pct      REAL,
  slo_window_days     INTEGER,
  slo_breached_at     INTEGER,
  agent_id            TEXT,
  last_run_started_at INTEGER,
  escalation_policy_id TEXT,
  org_id              TEXT    NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
  check_cert          INTEGER NOT NULL DEFAULT 0,
  cert_expiry_days    INTEGER NOT NULL DEFAULT 14
);
CREATE INDEX monitors_org_id_idx ON monitors(org_id);

CREATE TABLE heartbeats (
  monitor_id  TEXT    NOT NULL REFERENCES monitors(id) ON DELETE CASCADE,
  ts          INTEGER NOT NULL,
  status      TEXT    NOT NULL,
  latency_ms  INTEGER,
  status_code INTEGER,
  msg         TEXT,
  retries     INTEGER NOT NULL DEFAULT 0,
  important   INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (monitor_id, ts)
);
