-- Multi-DB P2 (MySQL) — SIEM detection rules + findings. Forked from PG/SQLite.
-- uuid→CHAR(36), bool→TINYINT, smallint/int→SMALLINT/INT, ts→BIGINT, UUID[]
-- channel_ids→LONGTEXT(JSON), jsonb condition→LONGTEXT(JSON). `body ~* regex`
-- degrades to a case-insensitive LIKE substring in the query layer (no regex).
--
-- `condition` is a MySQL reserved word → backticked in all DDL/DML. The
-- findings→rules link uses a REAL table-level FK (ON DELETE CASCADE) — unlike
-- the documentary inline refs elsewhere — because cascading finding cleanup on
-- rule delete is behaviorally important for a SIEM. NOT-NULL TEXT columns omit
-- literal DEFAULTs (need 8.0.13+); the writer always binds them.

CREATE TABLE detection_rules (
  id               CHAR(36)     NOT NULL PRIMARY KEY,
  name             VARCHAR(255) NOT NULL,
  description      TEXT         NOT NULL,
  enabled          TINYINT      NOT NULL DEFAULT 1,
  severity         VARCHAR(16)  NOT NULL DEFAULT 'medium'
                                CHECK (severity IN ('low', 'medium', 'high', 'critical')),
  service          VARCHAR(255) NOT NULL DEFAULT '',
  min_level        SMALLINT     NOT NULL DEFAULT 0,
  body_regex       TEXT         NOT NULL,
  attr_key         VARCHAR(255) NOT NULL DEFAULT '',
  attr_val         VARCHAR(512) NOT NULL DEFAULT '',
  threshold        INT          NOT NULL DEFAULT 1,
  window_seconds   INT          NOT NULL DEFAULT 300,
  cooldown_seconds INT          NOT NULL DEFAULT 0,
  group_by         VARCHAR(255) NOT NULL DEFAULT '',
  `condition`      LONGTEXT,
  channel_ids      LONGTEXT     NOT NULL,
  escalation_policy_id CHAR(36) REFERENCES escalation_policies(id) ON DELETE SET NULL,
  last_checked_at  BIGINT,
  created_at       BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  org_id           CHAR(36)     NOT NULL REFERENCES organizations(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX detection_rules_org_idx ON detection_rules (org_id);

CREATE TABLE detection_findings (
  id              CHAR(36)     NOT NULL PRIMARY KEY,
  rule_id         CHAR(36)     NOT NULL,
  rule_name       VARCHAR(255) NOT NULL,
  severity        VARCHAR(16)  NOT NULL
                               CHECK (severity IN ('low', 'medium', 'high', 'critical')),
  match_count     BIGINT       NOT NULL,
  sample          TEXT,
  service         VARCHAR(255),
  entity          VARCHAR(512),
  window_from     BIGINT       NOT NULL,
  window_to       BIGINT       NOT NULL,
  created_at      BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  acknowledged_at BIGINT,
  CONSTRAINT detection_findings_rule_fk
    FOREIGN KEY (rule_id) REFERENCES detection_rules(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX detection_findings_recent_idx ON detection_findings (created_at);
CREATE INDEX detection_findings_rule_idx ON detection_findings (rule_id, created_at);
