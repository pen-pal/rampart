-- Multi-DB P2 (SQLite) — error & exception tracking (Tier 1, Sentry-lite).
-- Ported from PG/MySQL. A project namespaces one app's errors; events group into
-- issues by (project_id, fingerprint) — the UNIQUE index is the grouping
-- invariant + the ON CONFLICT DO NOTHING upsert target. Events carry the
-- per-occurrence detail and age out via per-project retention; issues persist.
--
-- Dialect: uuid→TEXT, ts→INTEGER unix-seconds, JSONB→TEXT (json_extract reads it
-- for affected-users / stats), `release`→quoted (reserved word).

CREATE TABLE error_projects (
  id                TEXT    NOT NULL PRIMARY KEY,
  name              TEXT    NOT NULL,
  slug              TEXT    NOT NULL UNIQUE,
  public_key        TEXT    NOT NULL UNIQUE,
  platform          TEXT,
  retention_days    INTEGER NOT NULL DEFAULT 30,
  alert_channel_ids TEXT    NOT NULL,
  created_at        INTEGER NOT NULL DEFAULT (unixepoch()),
  org_id            TEXT    NOT NULL REFERENCES organizations(id)
);
CREATE INDEX error_projects_org_idx ON error_projects (org_id);

CREATE TABLE error_issues (
  id          TEXT    NOT NULL PRIMARY KEY,
  project_id  TEXT    NOT NULL,
  fingerprint TEXT    NOT NULL,
  title       TEXT    NOT NULL,
  culprit     TEXT,
  level       TEXT    NOT NULL DEFAULT 'error',
  status      TEXT    NOT NULL DEFAULT 'unresolved',
  first_seen  INTEGER NOT NULL DEFAULT (unixepoch()),
  last_seen   INTEGER NOT NULL DEFAULT (unixepoch()),
  times_seen  INTEGER NOT NULL DEFAULT 0,
  assignee    TEXT
);
CREATE UNIQUE INDEX error_issues_group_idx  ON error_issues (project_id, fingerprint);
CREATE INDEX        error_issues_recent_idx ON error_issues (project_id, last_seen);

CREATE TABLE error_events (
  id             TEXT    NOT NULL PRIMARY KEY,
  issue_id       TEXT    NOT NULL,
  project_id     TEXT    NOT NULL,
  ts             INTEGER NOT NULL,
  level          TEXT    NOT NULL DEFAULT 'error',
  message        TEXT,
  exception_type TEXT,
  culprit        TEXT,
  environment    TEXT,
  "release"      TEXT,
  server_name    TEXT,
  stacktrace     TEXT,
  context        TEXT,
  trace_id       TEXT
);
CREATE INDEX error_events_issue_idx ON error_events (project_id, issue_id, ts);
CREATE INDEX error_events_prune_idx ON error_events (project_id, ts);
CREATE INDEX error_events_trace_idx ON error_events (trace_id);
