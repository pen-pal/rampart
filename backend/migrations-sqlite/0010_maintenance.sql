-- Multi-DB P1 boot-wiring (SQLite) — maintenance windows.
--
-- Forked from PG 0004 (maintenance_windows + window_monitors), 0021 (recurrence
-- jsonb), 0050 (notified_start_at/end_at), 0108/0112 (org_id NOT NULL).
-- Dialect: uuid→TEXT, timestamptz→INTEGER unix-seconds, bool→INTEGER 0/1,
-- jsonb recurrence → TEXT (serde round-trip; default '{"kind":"none"}').

CREATE TABLE maintenance_windows (
  id                TEXT    PRIMARY KEY,
  name              TEXT    NOT NULL,
  description       TEXT,
  start_at          INTEGER NOT NULL,
  end_at            INTEGER NOT NULL,
  active            INTEGER NOT NULL DEFAULT 1,
  created_at        INTEGER NOT NULL DEFAULT (unixepoch()),
  recurrence        TEXT    NOT NULL DEFAULT '{"kind":"none"}',
  notified_start_at INTEGER,
  notified_end_at   INTEGER,
  org_id            TEXT    NOT NULL REFERENCES organizations(id),
  CHECK (end_at > start_at)
);
CREATE INDEX maintenance_windows_org_idx ON maintenance_windows (org_id);
CREATE INDEX maintenance_windows_active_idx ON maintenance_windows (active);

CREATE TABLE maintenance_window_monitors (
  window_id  TEXT NOT NULL REFERENCES maintenance_windows(id) ON DELETE CASCADE,
  monitor_id TEXT NOT NULL REFERENCES monitors(id)            ON DELETE CASCADE,
  PRIMARY KEY (window_id, monitor_id)
);
