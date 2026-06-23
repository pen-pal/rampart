-- Multi-DB P2 (MySQL) — core monitoring: monitors + heartbeats + agents + tags
-- and the tag join tables, forked from the PG/SQLite schema. These are one
-- migration because the domains are mutually dependent (monitors hydrate tags;
-- monitor_tags FKs monitors; the stale-agent watchdog JOINs agents + heartbeats).
-- FK order: organizations/users (already present) → agents/monitors → heartbeats,
-- tags → monitor_tags.
--
-- Dialect: uuid→CHAR(36), enums→VARCHAR (app-validated), int[] accepted_statuses
-- → LONGTEXT(JSON), jsonb→LONGTEXT, numeric slo_target_pct→DOUBLE, ts→BIGINT
-- unix-seconds, bool→TINYINT 0/1. All Monitor integer fields are i32 → INT.

CREATE TABLE agents (
  id           CHAR(36)     NOT NULL PRIMARY KEY,
  name         VARCHAR(255) NOT NULL,
  location     VARCHAR(255),
  token_hash   VARCHAR(128) NOT NULL UNIQUE,
  version      VARCHAR(64),
  last_seen_at BIGINT,
  created_at   BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  org_id       CHAR(36)     NOT NULL REFERENCES organizations(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX agents_org_idx ON agents (org_id);

CREATE TABLE monitors (
  id                   CHAR(36)     NOT NULL PRIMARY KEY,
  name                 VARCHAR(255) NOT NULL,
  kind                 VARCHAR(40)  NOT NULL,
  url                  TEXT,
  hostname             VARCHAR(255),
  port                 INT,
  config               LONGTEXT     NOT NULL DEFAULT ('{}'),
  interval_seconds     INT          NOT NULL,
  retry_interval_sec   INT          NOT NULL DEFAULT 60,
  max_retries          INT          NOT NULL DEFAULT 0,
  timeout_seconds      INT          NOT NULL,
  resend_interval_sec  INT          NOT NULL DEFAULT 0,
  upside_down          TINYINT      NOT NULL DEFAULT 0,
  http_method          VARCHAR(16)  NOT NULL DEFAULT 'GET',
  http_body            LONGTEXT,
  http_headers         LONGTEXT,
  accepted_statuses    LONGTEXT     NOT NULL DEFAULT ('[200]'),
  follow_redirect      TINYINT      NOT NULL DEFAULT 1,
  ignore_tls           TINYINT      NOT NULL DEFAULT 0,
  proxy_id             CHAR(36),
  active               TINYINT      NOT NULL DEFAULT 1,
  current_status       VARCHAR(16)  NOT NULL DEFAULT 'pending',
  created_at           BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  updated_at           BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  push_token           VARCHAR(64),
  last_push_at         BIGINT,
  cert_days_left       INT,
  cert_subject         TEXT,
  cert_checked_at      BIGINT,
  group_id             CHAR(36),
  slo_target_pct       DOUBLE,
  slo_window_days      INT,
  slo_breached_at      BIGINT,
  agent_id             CHAR(36),
  last_run_started_at  BIGINT,
  escalation_policy_id CHAR(36),
  org_id               CHAR(36)     NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
  check_cert           TINYINT      NOT NULL DEFAULT 0,
  cert_expiry_days     INT          NOT NULL DEFAULT 14
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX monitors_org_id_idx ON monitors (org_id);

CREATE TABLE heartbeats (
  monitor_id  CHAR(36)    NOT NULL REFERENCES monitors(id) ON DELETE CASCADE,
  ts          BIGINT      NOT NULL,
  status      VARCHAR(16) NOT NULL,
  latency_ms  INT,
  status_code INT,
  msg         TEXT,
  retries     INT         NOT NULL DEFAULT 0,
  important   TINYINT     NOT NULL DEFAULT 0,
  PRIMARY KEY (monitor_id, ts)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE tags (
  id         CHAR(36)     NOT NULL PRIMARY KEY,
  name       VARCHAR(190) NOT NULL,
  color      VARCHAR(32)  NOT NULL DEFAULT '#14b8a6',
  org_id     CHAR(36)     NOT NULL REFERENCES organizations(id),
  created_at BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  UNIQUE KEY tags_org_name_uidx (org_id, name)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE monitor_tags (
  monitor_id CHAR(36) NOT NULL REFERENCES monitors(id) ON DELETE CASCADE,
  tag_id     CHAR(36) NOT NULL REFERENCES tags(id)     ON DELETE CASCADE,
  value      TEXT,
  PRIMARY KEY (monitor_id, tag_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- notifications / monitor_groups not yet forked → no FK to them (would dangle);
-- they exist so tags::usage() can COUNT without a missing-table error.
CREATE TABLE notification_tags (
  notification_id CHAR(36) NOT NULL,
  tag_id          CHAR(36) NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  PRIMARY KEY (notification_id, tag_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE group_tags (
  group_id CHAR(36) NOT NULL,
  tag_id   CHAR(36) NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  PRIMARY KEY (group_id, tag_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
