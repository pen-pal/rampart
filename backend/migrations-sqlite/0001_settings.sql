-- Multi-DB P1 (SQLite tier) — parallel migration set. P1-0 spike: just the
-- settings table, to prove the SQLite toolchain (offline .sqlx cache +
-- #[sqlx::test] fixture). The full schema fork lands in later P1 slices.
--
-- SQLite dialect notes vs the Postgres schema (migrations/0001_initial.sql):
--   JSONB        -> TEXT   (JSON stored as text; SQLite has the json1 funcs)
--   TIMESTAMPTZ  -> TEXT   (ISO-8601 via datetime('now'))
CREATE TABLE settings (
  key         TEXT  PRIMARY KEY,
  value       TEXT  NOT NULL,
  updated_at  TEXT  NOT NULL DEFAULT (datetime('now'))
);
