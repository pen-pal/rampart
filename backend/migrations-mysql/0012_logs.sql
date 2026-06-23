-- Multi-DB P2 (MySQL) — log storage (telemetry foundation for detection +
-- telemetry_rules). Forked from PG/SQLite. The PG `body_tsv` generated tsvector
-- + GIN full-text index has no InnoDB-parity equivalent with the SAME semantics
-- (MATCH…AGAINST is word/stopword/min-token based, not substring), so — like
-- the SQLite tier — body search degrades to a `LIKE` substring match. Acceptable
-- for the non-PG tiers; add a FULLTEXT index + MATCH if word-search is wanted.
-- Dialect: uuid→CHAR(36), smallint→SMALLINT, jsonb→LONGTEXT, ts→BIGINT.

CREATE TABLE logs (
  id            CHAR(36)     NOT NULL PRIMARY KEY,
  ts            BIGINT       NOT NULL,
  severity      SMALLINT     NOT NULL DEFAULT 0,
  severity_text VARCHAR(64),
  service_name  VARCHAR(255) NOT NULL DEFAULT 'unknown',
  body          TEXT         NOT NULL,
  trace_id      VARCHAR(64),
  span_id       VARCHAR(64),
  attributes    LONGTEXT,
  received_at   BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  org_id        CHAR(36)     NOT NULL REFERENCES organizations(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX logs_recent_idx  ON logs (received_at);
CREATE INDEX logs_service_idx ON logs (service_name, received_at);
CREATE INDEX logs_org_idx      ON logs (org_id);
CREATE INDEX logs_trace_idx    ON logs (trace_id);
