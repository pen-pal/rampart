-- Multi-DB P2 (MySQL) management-API tail — on-call schedules + web-push
-- subscriptions (closes the 2 feature-conditional notifier-dispatch gaps).
-- Ported from PG (+ 0093 participant_user_ids). uuid→CHAR(36), JSONB→LONGTEXT,
-- ts→BIGINT. endpoint→VARCHAR(512) UNIQUE (covers real push endpoints, under the
-- InnoDB key-length limit) so the upsert's ON DUPLICATE KEY can dedup on it.
-- VAPID keypair is NOT a table — it lives in `settings` under webpush_vapid.

CREATE TABLE on_call_schedules (
  id                   CHAR(36)     NOT NULL PRIMARY KEY,
  name                 VARCHAR(255) NOT NULL,
  rotation_seconds     BIGINT       NOT NULL,
  anchor               BIGINT       NOT NULL,
  participant_ids      LONGTEXT     NOT NULL,
  participant_user_ids LONGTEXT     NOT NULL,
  created_at           BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  org_id               CHAR(36)     NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX on_call_schedules_org_idx ON on_call_schedules (org_id);

CREATE TABLE webpush_subscriptions (
  id              CHAR(36)     NOT NULL PRIMARY KEY,
  notification_id CHAR(36)     NOT NULL,
  endpoint        VARCHAR(512) NOT NULL UNIQUE,
  p256dh          TEXT         NOT NULL,
  auth            TEXT         NOT NULL,
  created_at      BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP())
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX webpush_subs_notif_idx ON webpush_subscriptions (notification_id);
