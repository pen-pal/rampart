-- Multi-DB P2 (MySQL) — threshold/anomaly alert rules over ingested metrics.
-- Forked from PG/SQLite. uuid→CHAR(36), jsonb labels + UUID[] channel_ids→
-- LONGTEXT(JSON), double→DOUBLE, ts→BIGINT, enabled→TINYINT, op→VARCHAR with a
-- CHECK mirroring the PG enum (RuleOp::as_str only emits these; from_db tolerates
-- unknown). NOT-NULL TEXT columns omit literal DEFAULTs (TEXT defaults need MySQL
-- 8.0.13+); the writer always binds labels/channel_ids.

CREATE TABLE metric_rules (
  id                   CHAR(36)     NOT NULL PRIMARY KEY,
  name                 VARCHAR(255) NOT NULL,
  metric               VARCHAR(255) NOT NULL,
  labels               LONGTEXT     NOT NULL,
  op                   VARCHAR(16)  NOT NULL CHECK (op IN ('gt', 'lt', 'gte', 'lte', 'anomaly')),
  threshold            DOUBLE       NOT NULL,
  for_seconds          INT          NOT NULL DEFAULT 0 CHECK (for_seconds >= 0),
  enabled              TINYINT      NOT NULL DEFAULT 1,
  channel_ids          LONGTEXT     NOT NULL,
  escalation_policy_id CHAR(36)     REFERENCES escalation_policies(id) ON DELETE SET NULL,
  breach_since         BIGINT,
  firing_at            BIGINT,
  created_at           BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  org_id               CHAR(36)     NOT NULL REFERENCES organizations(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX metric_rules_org_idx ON metric_rules (org_id);
