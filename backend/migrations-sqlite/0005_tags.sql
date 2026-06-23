-- Multi-DB P1 (SQLite) — tags + the join tables that hang off them.
--
-- Forked from PG migrations 0001 (tags, monitor_tags), 0027 (routing join
-- tables), 0108 (org_id), 0113 (per-org name uniqueness). Dialect map (see
-- migrations-sqlite/0002_identity.sql): uuid → TEXT, timestamptz → INTEGER
-- unix-seconds (`unixepoch()`), per-org name uniqueness as a UNIQUE INDEX
-- (PG's tags_org_name_uidx, replacing the global tags_name_key).

CREATE TABLE tags (
  id         TEXT    PRIMARY KEY,
  name       TEXT    NOT NULL,
  color      TEXT    NOT NULL DEFAULT '#14b8a6',
  org_id     TEXT    NOT NULL DEFAULT '00000000-0000-0000-0000-000000000001'
                     REFERENCES organizations(id),
  created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

-- Per-org unique name (PG 0113: tags_org_name_uidx). The create() conflict
-- mapper turns a violation here into DbError::Conflict.
CREATE UNIQUE INDEX tags_org_name_uidx ON tags (org_id, name);

CREATE TABLE monitor_tags (
  monitor_id TEXT NOT NULL REFERENCES monitors(id) ON DELETE CASCADE,
  tag_id     TEXT NOT NULL REFERENCES tags(id)     ON DELETE CASCADE,
  value      TEXT,
  PRIMARY KEY (monitor_id, tag_id)
);

-- Routing join tables. The parent `notifications` / `monitor_groups` tables are
-- not yet forked into the SQLite schema, so these are declared WITHOUT a FK to
-- them (a forward FK would dangle); they exist so `tags::usage()` can COUNT
-- channel/group attachments without a missing-table error. FKs get added when
-- those domains land.
CREATE TABLE notification_tags (
  notification_id TEXT NOT NULL,
  tag_id          TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  PRIMARY KEY (notification_id, tag_id)
);

CREATE TABLE group_tags (
  group_id TEXT NOT NULL,
  tag_id   TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  PRIMARY KEY (group_id, tag_id)
);
