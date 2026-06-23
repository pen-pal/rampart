-- Multi-DB P2 (MySQL) — maintenance windows + their monitor sets (scheduler/
-- notifier-tail slice toward a panic-free mysql:// boot — un-stubs the scheduler
-- tick's is_in_active_window + transitions_needing_notification). Forked from
-- PG/SQLite. uuid→CHAR(36), ts→BIGINT, bool→TINYINT, jsonb recurrence→LONGTEXT
-- (serde round-trip; the writer always binds it). Real ON DELETE CASCADE FKs on
-- the join table (the inline org_id ref stays documentary, per convention).

CREATE TABLE maintenance_windows (
  id                CHAR(36)     NOT NULL PRIMARY KEY,
  name              VARCHAR(255) NOT NULL,
  description       TEXT,
  start_at          BIGINT       NOT NULL,
  end_at            BIGINT       NOT NULL,
  active            TINYINT      NOT NULL DEFAULT 1,
  created_at        BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  recurrence        LONGTEXT     NOT NULL,
  notified_start_at BIGINT,
  notified_end_at   BIGINT,
  org_id            CHAR(36)     NOT NULL REFERENCES organizations(id),
  CHECK (end_at > start_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX maintenance_windows_org_idx ON maintenance_windows (org_id);
CREATE INDEX maintenance_windows_active_idx ON maintenance_windows (active);

CREATE TABLE maintenance_window_monitors (
  window_id  CHAR(36) NOT NULL,
  monitor_id CHAR(36) NOT NULL,
  PRIMARY KEY (window_id, monitor_id),
  CONSTRAINT mwm_window_fk  FOREIGN KEY (window_id)  REFERENCES maintenance_windows(id) ON DELETE CASCADE,
  CONSTRAINT mwm_monitor_fk FOREIGN KEY (monitor_id) REFERENCES monitors(id)            ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
