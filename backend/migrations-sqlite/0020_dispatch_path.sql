-- Multi-DB P1 domain-port tail (SQLite) — the notifier dispatch-path tables.
--
-- The notifier's per-event dispatch reads three tables that weren't yet forked:
-- monitor_groups (folder tree, for tag/folder channel routing), its self-M2M
-- monitor_dependencies (dependency suppression: any_parent_down), and silences
-- (alert suppression: is_silenced). Forked from PG 0022 + 0031 (parent_id) +
-- the silences migration + 0108 (org_id). Dialect: uuid→TEXT, ts→INTEGER
-- unix-seconds. Tables only — CRUD stays stubbed until the management-API slices
-- land; the dispatch READS (routing/monitor_groups/silences) are wired now.

CREATE TABLE monitor_groups (
  id         TEXT    PRIMARY KEY,
  name       TEXT    NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0,
  parent_id  TEXT    REFERENCES monitor_groups(id) ON DELETE SET NULL,
  created_at INTEGER NOT NULL DEFAULT (unixepoch()),
  org_id     TEXT    NOT NULL REFERENCES organizations(id)
);
CREATE INDEX monitor_groups_parent_idx ON monitor_groups (parent_id);
CREATE INDEX monitor_groups_org_idx ON monitor_groups (org_id);

CREATE TABLE monitor_dependencies (
  monitor_id    TEXT    NOT NULL REFERENCES monitors(id) ON DELETE CASCADE,
  depends_on_id TEXT    NOT NULL REFERENCES monitors(id) ON DELETE CASCADE,
  created_at    INTEGER NOT NULL DEFAULT (unixepoch()),
  PRIMARY KEY (monitor_id, depends_on_id),
  CHECK (monitor_id <> depends_on_id)
);
CREATE INDEX monitor_dependencies_parent_idx ON monitor_dependencies (depends_on_id);

CREATE TABLE silences (
  id         TEXT    PRIMARY KEY,
  monitor_id TEXT    REFERENCES monitors(id) ON DELETE CASCADE,
  reason     TEXT    NOT NULL DEFAULT '',
  created_by TEXT    REFERENCES users(id) ON DELETE SET NULL,
  created_at INTEGER NOT NULL DEFAULT (unixepoch()),
  expires_at INTEGER,
  org_id     TEXT    NOT NULL REFERENCES organizations(id)
);
CREATE INDEX silences_active_idx ON silences (expires_at, monitor_id);
