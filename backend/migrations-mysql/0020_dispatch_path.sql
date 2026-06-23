-- Multi-DB P2 (MySQL) domain-port tail — the notifier dispatch-path tables.
-- The notifier's per-event dispatch reads three tables not yet forked to MySQL:
-- monitor_groups (folder tree, for tag/folder channel routing), its
-- monitor_dependencies (dependency suppression: any_parent_down), and silences
-- (alert suppression: is_silenced). Forked from PG/SQLite. uuid→CHAR(36),
-- ts→BIGINT. Tables only — CRUD stays stubbed; the dispatch READS
-- (routing/monitor_groups/silences/templates) are wired now. (notification_
-- templates + monitor_notifications/group_notifications/excludes already exist
-- in 0006; agents in 0004.) Real CASCADE/SET-NULL FKs on the join/dep tables.

CREATE TABLE monitor_groups (
  id         CHAR(36)     NOT NULL PRIMARY KEY,
  name       VARCHAR(255) NOT NULL,
  sort_order INT          NOT NULL DEFAULT 0,
  parent_id  CHAR(36),
  created_at BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  org_id     CHAR(36)     NOT NULL,
  CONSTRAINT monitor_groups_parent_fk
    FOREIGN KEY (parent_id) REFERENCES monitor_groups(id) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX monitor_groups_parent_idx ON monitor_groups (parent_id);
CREATE INDEX monitor_groups_org_idx ON monitor_groups (org_id);

CREATE TABLE monitor_dependencies (
  monitor_id    CHAR(36) NOT NULL,
  depends_on_id CHAR(36) NOT NULL,
  created_at    BIGINT   NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  PRIMARY KEY (monitor_id, depends_on_id),
  CHECK (monitor_id <> depends_on_id),
  CONSTRAINT monitor_dependencies_child_fk
    FOREIGN KEY (monitor_id) REFERENCES monitors(id) ON DELETE CASCADE,
  CONSTRAINT monitor_dependencies_parent_fk
    FOREIGN KEY (depends_on_id) REFERENCES monitors(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX monitor_dependencies_parent_idx ON monitor_dependencies (depends_on_id);

CREATE TABLE silences (
  id         CHAR(36) NOT NULL PRIMARY KEY,
  monitor_id CHAR(36),
  reason     TEXT,
  created_by CHAR(36),
  created_at BIGINT   NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  expires_at BIGINT,
  org_id     CHAR(36) NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX silences_active_idx ON silences (expires_at, monitor_id);
