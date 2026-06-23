-- Multi-DB P2 (MySQL) — trace spans (telemetry foundation 2/2). Forked from
-- PG/SQLite. hex ids→VARCHAR, smallint→SMALLINT, ns→BIGINT, double→DOUBLE,
-- jsonb attrs→LONGTEXT, received_at→BIGINT. span_id PRIMARY KEY backs the insert
-- `ON DUPLICATE KEY UPDATE span_id = span_id` (no-op → 0 affected rows on a
-- retransmit, so the inserted-count stays exact, mirroring PG's DO NOTHING).

CREATE TABLE spans (
  span_id        VARCHAR(64)  NOT NULL PRIMARY KEY,
  trace_id       VARCHAR(64)  NOT NULL,
  parent_span_id VARCHAR(64),
  service_name   VARCHAR(255) NOT NULL DEFAULT 'unknown',
  name           VARCHAR(512) NOT NULL,
  kind           SMALLINT     NOT NULL DEFAULT 0,
  start_ns       BIGINT       NOT NULL,
  end_ns         BIGINT       NOT NULL,
  duration_ms    DOUBLE       NOT NULL,
  status_code    SMALLINT     NOT NULL DEFAULT 0,
  status_message TEXT,
  attributes     LONGTEXT,
  received_at    BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  org_id         CHAR(36)     NOT NULL REFERENCES organizations(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX spans_recent_idx  ON spans (received_at);
CREATE INDEX spans_service_idx ON spans (service_name, received_at);
CREATE INDEX spans_trace_idx   ON spans (trace_id);
CREATE INDEX spans_org_idx      ON spans (org_id);
