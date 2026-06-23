-- Multi-DB P2 (MySQL) — durable backing store for the notifier's per-channel
-- digest buffer (the scheduler/notifier-tail slice that un-stubs StoreDigestBuffer
-- so a mysql:// boot's notifier digest-flush timer no longer panics). Forked from
-- PG/SQLite. uuid→CHAR(36), jsonb event_json→LONGTEXT, enqueued_at→BIGINT. A real
-- ON DELETE CASCADE FK drops buffered events when their channel is deleted.

CREATE TABLE digest_buffer (
  id              CHAR(36) NOT NULL PRIMARY KEY,
  notification_id CHAR(36) NOT NULL,
  event_json      LONGTEXT NOT NULL,
  enqueued_at     BIGINT   NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  CONSTRAINT digest_buffer_notif_fk
    FOREIGN KEY (notification_id) REFERENCES notifications(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX digest_buffer_notif_idx ON digest_buffer (notification_id, enqueued_at);
