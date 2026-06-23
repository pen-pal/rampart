-- Multi-DB P2 (MySQL) — telemetry alert rules over the telemetry tiers. Forked
-- from PG/SQLite. uuid→CHAR(36), smallint/int→SMALLINT/INT, double→DOUBLE,
-- UUID[] channel_ids→LONGTEXT(JSON), ts→BIGINT, enabled→TINYINT. The `op` CHECK
-- ports verbatim; `kind` is app-validated via the TelemetryRuleKind serde
-- round-trip (same precedent as monitors.kind). NOT-NULL channel_ids omits a
-- literal default (TEXT defaults need 8.0.13+); the writer always binds it.

CREATE TABLE telemetry_alert_rules (
  id                   CHAR(36)     NOT NULL PRIMARY KEY,
  name                 VARCHAR(255) NOT NULL,
  kind                 VARCHAR(32)  NOT NULL,
  target               VARCHAR(255) NOT NULL DEFAULT '',
  match_text           VARCHAR(255) NOT NULL DEFAULT '',
  min_level            SMALLINT     NOT NULL DEFAULT 0,
  op                   VARCHAR(16)  NOT NULL CHECK (op IN ('gt', 'lt', 'gte', 'lte')),
  threshold            DOUBLE       NOT NULL,
  window_seconds       INT          NOT NULL DEFAULT 300 CHECK (window_seconds > 0),
  for_seconds          INT          NOT NULL DEFAULT 0 CHECK (for_seconds >= 0),
  enabled              TINYINT      NOT NULL DEFAULT 1,
  channel_ids          LONGTEXT     NOT NULL,
  escalation_policy_id CHAR(36)     REFERENCES escalation_policies(id) ON DELETE SET NULL,
  breach_since         BIGINT,
  firing_at            BIGINT,
  created_at           BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  org_id               CHAR(36)     NOT NULL REFERENCES organizations(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX telemetry_alert_rules_org_idx ON telemetry_alert_rules (org_id);
