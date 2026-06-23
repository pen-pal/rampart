-- Multi-DB P2 (MySQL) — service level objectives. Forked from PG/SQLite.
-- uuid→CHAR(36), jsonb labels + UUID[] channel_ids→LONGTEXT(JSON), double→DOUBLE,
-- ts→BIGINT, enabled→TINYINT. CHECKs (sli_kind, objective_pct range, window_days
-- range) port verbatim (enforced on MariaDB). NOT-NULL TEXT columns omit literal
-- DEFAULTs (need 8.0.13+); the writer always binds description/labels/channel_ids.

CREATE TABLE slos (
  id                   CHAR(36)     NOT NULL PRIMARY KEY,
  name                 VARCHAR(255) NOT NULL,
  description          TEXT         NOT NULL,
  sli_kind             VARCHAR(16)  NOT NULL CHECK (sli_kind IN ('monitor', 'metric')),
  monitor_id           CHAR(36)     REFERENCES monitors(id) ON DELETE CASCADE,
  good_metric          VARCHAR(255),
  total_metric         VARCHAR(255),
  labels               LONGTEXT     NOT NULL,
  objective_pct        DOUBLE       NOT NULL CHECK (objective_pct > 0 AND objective_pct < 100),
  window_days          INT          NOT NULL DEFAULT 30 CHECK (window_days BETWEEN 1 AND 365),
  enabled              TINYINT      NOT NULL DEFAULT 1,
  channel_ids          LONGTEXT     NOT NULL,
  escalation_policy_id CHAR(36)     REFERENCES escalation_policies(id) ON DELETE SET NULL,
  breaching_at         BIGINT,
  created_at           BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  org_id               CHAR(36)     NOT NULL REFERENCES organizations(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX slos_org_idx ON slos (org_id);
