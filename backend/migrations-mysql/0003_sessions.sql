-- Multi-DB P2 (MySQL) — server-side sessions (cookie value = session id).
-- Forked from the PG/SQLite sessions table. uuid→CHAR(36), ts→BIGINT unix, IP
-- stored as plain VARCHAR. Session ids are random v4 UUIDs (app-side).

CREATE TABLE sessions (
  id            CHAR(36)    NOT NULL PRIMARY KEY,
  user_id       CHAR(36)    NOT NULL,
  created_at    BIGINT      NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  expires_at    BIGINT      NOT NULL,
  ip_addr       VARCHAR(64),
  user_agent    TEXT,
  active_org_id CHAR(36),
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX sessions_user_id_idx ON sessions (user_id);
