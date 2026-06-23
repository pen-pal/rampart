-- Multi-DB P2 (MySQL) — append-only record of every channel send attempt.
-- Forked from PG/SQLite. BIGSERIAL PK → BIGINT AUTO_INCREMENT (record() reads it
-- back via LAST_INSERT_ID, no RETURNING). org floored to the channel's org (or
-- Default) in-SQL on insert. uuid→CHAR(36), bool→TINYINT, ts→BIGINT.

CREATE TABLE delivery_log (
  id              BIGINT      NOT NULL AUTO_INCREMENT PRIMARY KEY,
  notification_id CHAR(36)    REFERENCES notifications(id) ON DELETE SET NULL,
  channel_kind    VARCHAR(32) NOT NULL,
  event_kind      VARCHAR(32) NOT NULL,
  monitor_id      CHAR(36),
  ok              TINYINT     NOT NULL,
  error           TEXT,
  org_id          CHAR(36)    NOT NULL REFERENCES organizations(id),
  sent_at         BIGINT      NOT NULL DEFAULT (UNIX_TIMESTAMP())
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX delivery_log_org_sent_idx ON delivery_log (org_id, sent_at, id);
