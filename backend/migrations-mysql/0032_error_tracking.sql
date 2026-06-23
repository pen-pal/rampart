-- Multi-DB P2 (MySQL) — error & exception tracking (Tier 1, Sentry-lite).
-- Ported from PG. A project namespaces one app's errors; events group into
-- issues by (project_id, fingerprint) — the UNIQUE index is the grouping
-- invariant + the INSERT-IGNORE upsert target. Events carry the per-occurrence
-- detail and age out via per-project retention; issues persist.
--
-- uuid→CHAR(36), ts→BIGINT, JSONB→JSON (native, so the affected-users / stats
-- reads can use JSON-path extraction), `release`→backticked (reserved word).
-- alert_channel_ids JSONB→LONGTEXT (a serde array, not value-queried).

CREATE TABLE error_projects (
  id                CHAR(36)     NOT NULL PRIMARY KEY,
  name              VARCHAR(255) NOT NULL,
  slug              VARCHAR(255) NOT NULL UNIQUE,
  public_key        VARCHAR(64)  NOT NULL UNIQUE,
  platform          VARCHAR(64)  NULL,
  retention_days    INT          NOT NULL DEFAULT 30,
  alert_channel_ids LONGTEXT     NOT NULL,
  created_at        BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  org_id            CHAR(36)     NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX error_projects_org_idx ON error_projects (org_id);

CREATE TABLE error_issues (
  id          CHAR(36)     NOT NULL PRIMARY KEY,
  project_id  CHAR(36)     NOT NULL,
  fingerprint VARCHAR(255) NOT NULL,
  title       TEXT         NOT NULL,
  culprit     TEXT         NULL,
  level       VARCHAR(32)  NOT NULL DEFAULT 'error',
  status      VARCHAR(32)  NOT NULL DEFAULT 'unresolved',
  first_seen  BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  last_seen   BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  times_seen  BIGINT       NOT NULL DEFAULT 0,
  assignee    CHAR(36)     NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE UNIQUE INDEX error_issues_group_idx  ON error_issues (project_id, fingerprint);
CREATE INDEX        error_issues_recent_idx ON error_issues (project_id, last_seen);

CREATE TABLE error_events (
  id             CHAR(36)     NOT NULL PRIMARY KEY,
  issue_id       CHAR(36)     NOT NULL,
  project_id     CHAR(36)     NOT NULL,
  ts             BIGINT       NOT NULL,
  level          VARCHAR(32)  NOT NULL DEFAULT 'error',
  message        TEXT         NULL,
  exception_type VARCHAR(255) NULL,
  culprit        TEXT         NULL,
  environment    VARCHAR(255) NULL,
  `release`      VARCHAR(255) NULL,
  server_name    VARCHAR(255) NULL,
  stacktrace     JSON         NULL,
  context        JSON         NULL,
  trace_id       VARCHAR(64)  NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX error_events_issue_idx ON error_events (project_id, issue_id, ts);
CREATE INDEX error_events_prune_idx ON error_events (project_id, ts);
CREATE INDEX error_events_trace_idx ON error_events (trace_id);
