-- Multi-DB P2 (MySQL) — notification channels + templates + the monitor/group
-- attach + exclude join tables. Forked from PG/SQLite. Channel `config` holds
-- secrets sealed app-side (crate::secrets); stored as LONGTEXT(JSON). uuid→
-- CHAR(36), bool→TINYINT, ts→BIGINT, int fields→INT. notification_tags/
-- group_tags already exist (0004). digest_buffer + delivery_log land with the
-- notifier slice.

CREATE TABLE notification_templates (
  id               CHAR(36)     NOT NULL PRIMARY KEY,
  name             VARCHAR(255) NOT NULL,
  channel_kinds    LONGTEXT     NOT NULL,
  event_kind       VARCHAR(64)  NOT NULL,
  subject_template TEXT,
  body_template    LONGTEXT     NOT NULL,
  is_default       TINYINT      NOT NULL DEFAULT 0,
  org_id           CHAR(36)     NOT NULL REFERENCES organizations(id),
  created_at       BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP())
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE notifications (
  id                  CHAR(36)     NOT NULL PRIMARY KEY,
  name                VARCHAR(255) NOT NULL,
  kind                VARCHAR(32)  NOT NULL,
  config              LONGTEXT     NOT NULL,
  is_default          TINYINT      NOT NULL DEFAULT 0,
  template_id         CHAR(36)     REFERENCES notification_templates(id) ON DELETE SET NULL,
  active              TINYINT      NOT NULL DEFAULT 1,
  last_fired_at       BIGINT,
  cooldown_seconds    INT          NOT NULL DEFAULT 0,
  digest_window_secs  INT          NOT NULL DEFAULT 0
                        CHECK (digest_window_secs >= 0 AND digest_window_secs <= 3600),
  quiet_hours_start   INT          CHECK (quiet_hours_start IS NULL
                        OR (quiet_hours_start >= 0 AND quiet_hours_start <= 23)),
  quiet_hours_end     INT          CHECK (quiet_hours_end IS NULL
                        OR (quiet_hours_end >= 0 AND quiet_hours_end <= 23)),
  rate_limit_per_hour INT          NOT NULL DEFAULT 0 CHECK (rate_limit_per_hour >= 0),
  org_id              CHAR(36)     NOT NULL REFERENCES organizations(id),
  created_at          BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP())
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX notifications_org_idx ON notifications (org_id);

CREATE TABLE monitor_notifications (
  monitor_id      CHAR(36) NOT NULL REFERENCES monitors(id)      ON DELETE CASCADE,
  notification_id CHAR(36) NOT NULL REFERENCES notifications(id) ON DELETE CASCADE,
  PRIMARY KEY (monitor_id, notification_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE group_notifications (
  group_id        CHAR(36) NOT NULL,
  notification_id CHAR(36) NOT NULL REFERENCES notifications(id) ON DELETE CASCADE,
  PRIMARY KEY (group_id, notification_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE monitor_notification_excludes (
  monitor_id      CHAR(36) NOT NULL REFERENCES monitors(id)      ON DELETE CASCADE,
  notification_id CHAR(36) NOT NULL REFERENCES notifications(id) ON DELETE CASCADE,
  PRIMARY KEY (monitor_id, notification_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
