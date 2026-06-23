-- Multi-DB P1 (SQLite) — notification channels, routing joins, digest buffer,
-- delivery log.
--
-- Forked from PG migrations:
--   0001 (notification_templates, notifications, monitor_notifications)
--   0020 (notifications.cooldown_seconds)
--   0053 (notifications.digest_window_secs + range CHECK)
--   0055 (digest_buffer)
--   0060 (notifications quiet_hours_* / rate_limit_per_hour + range CHECKs)
--   0065 (delivery_log)
--   0027 (group_notifications, monitor_notification_excludes — the two routing
--         joins NOT already created in 0005_tags.sql)
--   0108/0112 (org_id, baked at the NOT-NULL/no-default end-state) + 0113
--         (per-org uniqueness of notification_templates.name)
--
-- Dialect map (see 0004_monitors.sql / 0005_tags.sql): uuid → TEXT;
-- timestamptz → INTEGER unix-seconds (DEFAULT (unixepoch())); boolean →
-- INTEGER 0/1; jsonb → TEXT; channel_kind enum → plain TEXT, app-validated
-- (same precedent as monitors.kind — 142 kinds make an IN(...) CHECK brittle);
-- SMALLINT/INT → INTEGER. notification_tags + group_tags already exist (FK-less)
-- in 0005_tags.sql and are deliberately NOT redefined here.

-- ── notification templates (declared first: notifications.template_id refs it) ─
CREATE TABLE notification_templates (
  id                TEXT    PRIMARY KEY,
  name              TEXT    NOT NULL,
  channel_kinds     TEXT    NOT NULL,                 -- PG TEXT[] → JSON array TEXT
  event_kind        TEXT    NOT NULL,                 -- app-validated (no CHECK)
  subject_template  TEXT,
  body_template     TEXT    NOT NULL,
  is_default        INTEGER NOT NULL DEFAULT 0,
  org_id            TEXT    NOT NULL REFERENCES organizations(id),
  created_at        INTEGER NOT NULL DEFAULT (unixepoch())
);
-- PG 0113: global name UNIQUE replaced by per-org uniqueness.
CREATE UNIQUE INDEX notification_templates_org_name_uidx
  ON notification_templates (org_id, name);

-- ── notification channels ─────────────────────────────────────────────────────
CREATE TABLE notifications (
  id                  TEXT    PRIMARY KEY,
  name                TEXT    NOT NULL,
  kind                TEXT    NOT NULL,               -- channel_kind, app-validated
  config              TEXT    NOT NULL,               -- jsonb; secrets sealed app-side
  is_default          INTEGER NOT NULL DEFAULT 0,     -- present in PG; never r/w by this module
  template_id         TEXT    REFERENCES notification_templates(id) ON DELETE SET NULL,
  active              INTEGER NOT NULL DEFAULT 1,
  last_fired_at       INTEGER,
  cooldown_seconds    INTEGER NOT NULL DEFAULT 0,     -- 0020
  digest_window_secs  INTEGER NOT NULL DEFAULT 0      -- 0053
                      CHECK (digest_window_secs >= 0 AND digest_window_secs <= 3600),
  quiet_hours_start   INTEGER                         -- 0060
                      CHECK (quiet_hours_start IS NULL OR (quiet_hours_start >= 0 AND quiet_hours_start <= 23)),
  quiet_hours_end     INTEGER
                      CHECK (quiet_hours_end IS NULL OR (quiet_hours_end >= 0 AND quiet_hours_end <= 23)),
  rate_limit_per_hour INTEGER NOT NULL DEFAULT 0
                      CHECK (rate_limit_per_hour >= 0),
  org_id              TEXT    NOT NULL REFERENCES organizations(id),  -- 0108/0112
  created_at          INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX notifications_org_id_idx ON notifications (org_id);

-- ── monitor ↔ channel direct wiring (0001) ────────────────────────────────────
CREATE TABLE monitor_notifications (
  monitor_id      TEXT NOT NULL REFERENCES monitors(id)      ON DELETE CASCADE,
  notification_id TEXT NOT NULL REFERENCES notifications(id) ON DELETE CASCADE,
  PRIMARY KEY (monitor_id, notification_id)
);

-- ── tag-routing joins not already in 0005 (0027) ──────────────────────────────
-- group_notifications.group_id → monitor_groups, which is NOT yet forked: FK-less
-- (forward FK would dangle). FK lands when the groups domain is forked.
CREATE TABLE group_notifications (
  group_id        TEXT NOT NULL,
  notification_id TEXT NOT NULL REFERENCES notifications(id) ON DELETE CASCADE,
  PRIMARY KEY (group_id, notification_id)
);
CREATE INDEX group_notifications_notif_idx ON group_notifications (notification_id);

-- Per-monitor exclusion set; always wins over every inclusion path.
CREATE TABLE monitor_notification_excludes (
  monitor_id      TEXT NOT NULL REFERENCES monitors(id)      ON DELETE CASCADE,
  notification_id TEXT NOT NULL REFERENCES notifications(id) ON DELETE CASCADE,
  PRIMARY KEY (monitor_id, notification_id)
);

-- ── durable digest buffer (0055) ──────────────────────────────────────────────
-- Global/transient infra: deliberately NO org_id (PG 0108 leak note). CRUD lives
-- in rampart-notifier; the table exists now so a later sqlite/digest slice lands
-- migration-free.
CREATE TABLE digest_buffer (
  id              TEXT    PRIMARY KEY,
  notification_id TEXT    NOT NULL REFERENCES notifications(id) ON DELETE CASCADE,
  event_json      TEXT    NOT NULL,                  -- jsonb
  enqueued_at     INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX idx_digest_buffer_notification_id ON digest_buffer (notification_id);

-- ── delivery log (0065 + 0108/0112 org_id) ────────────────────────────────────
-- Append-only send record. PG BIGSERIAL PK → SQLite INTEGER PRIMARY KEY (rowid
-- alias, auto-increments — read as i64). notification_id FK is ON DELETE SET NULL
-- (the log outlives the channel); monitor_id has no FK (matches PG — may name a
-- deleted monitor). tenant_root: carries its own org_id (NOT NULL, no default;
-- record() always supplies it via COALESCE(..., Default)).
CREATE TABLE delivery_log (
  id              INTEGER PRIMARY KEY,
  notification_id TEXT    REFERENCES notifications(id) ON DELETE SET NULL,
  channel_kind    TEXT    NOT NULL,
  event_kind      TEXT    NOT NULL,
  monitor_id      TEXT,
  ok              INTEGER NOT NULL,                   -- boolean 0/1
  error           TEXT,
  org_id          TEXT    NOT NULL REFERENCES organizations(id),
  sent_at         INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX idx_delivery_log_sent_at ON delivery_log (sent_at DESC);
CREATE INDEX idx_delivery_log_org_id  ON delivery_log (org_id);
