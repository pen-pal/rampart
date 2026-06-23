-- Server-side sessions (cookie value = session id). Forked from the Postgres
-- sessions table (migrations/0001_initial.sql + the active_org_id column from
-- the multi-tenancy phase).
--   uuid        -> TEXT
--   inet        -> TEXT (plain ip string; PG used host(ip_addr) on read)
--   timestamptz -> INTEGER unix-seconds
CREATE TABLE sessions (
  id            TEXT    PRIMARY KEY,
  user_id       TEXT    NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  created_at    INTEGER NOT NULL DEFAULT (unixepoch()),
  expires_at    INTEGER NOT NULL,
  ip_addr       TEXT,
  user_agent    TEXT,
  active_org_id TEXT
);

CREATE INDEX sessions_user_id_idx ON sessions(user_id);
