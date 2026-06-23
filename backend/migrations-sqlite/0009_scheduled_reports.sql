-- Multi-DB P1 boot-wiring (SQLite) — scheduled uptime reports.
--
-- Forked from PG 0062 (scheduled_reports) + 0108/0112 (org_id NOT NULL).
-- Dialect: uuid→TEXT, TEXT[] recipients → JSON array TEXT (default '[]'),
-- timestamptz→INTEGER unix-seconds.

CREATE TABLE scheduled_reports (
  id           TEXT    PRIMARY KEY,
  name         TEXT    NOT NULL,
  recipients   TEXT    NOT NULL DEFAULT '[]',
  cadence      TEXT    NOT NULL DEFAULT 'weekly',
  last_sent_at INTEGER,
  created_at   INTEGER NOT NULL DEFAULT (unixepoch()),
  org_id       TEXT    NOT NULL REFERENCES organizations(id)
);
CREATE INDEX scheduled_reports_org_idx ON scheduled_reports (org_id);
