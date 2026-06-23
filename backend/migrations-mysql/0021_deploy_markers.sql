-- Multi-DB P2 (MySQL) management-API tail — deploy markers (timeline annotations
-- on charts). Forked from PG (deploy_markers + org_id NOT NULL). uuid→CHAR(36),
-- timestamptz→BIGINT unix-seconds, text→VARCHAR/TEXT. No SQLite reference (this
-- domain is a cold stub on SQLite too) — ported from the PG impl directly.

CREATE TABLE deploy_markers (
  id          CHAR(36)     NOT NULL PRIMARY KEY,
  ts          BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  title       VARCHAR(255) NOT NULL,
  description TEXT,
  service     VARCHAR(255),
  created_at  BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  org_id      CHAR(36)     NOT NULL REFERENCES organizations(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX deploy_markers_ts_idx ON deploy_markers (ts);
CREATE INDEX deploy_markers_org_idx ON deploy_markers (org_id);
