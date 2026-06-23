-- Multi-DB P1 boot-wiring (SQLite) — escalation policies + episodes.
--
-- Forked from PG 0074 (policies + episodes + partial unique index), 0096
-- (subject_kind/subject_ref generalization, monitor_id nullable, per-subject
-- open index), 0108/0112 (policies org_id NOT NULL). Dialect: uuid→TEXT, jsonb
-- steps→TEXT, timestamptz→INTEGER unix-seconds, int→INTEGER. The partial unique
-- index ("one open episode per subject") ports verbatim — SQLite supports
-- partial indexes + partial-target ON CONFLICT (3.35+).

CREATE TABLE escalation_policies (
  id         TEXT    PRIMARY KEY,
  name       TEXT    NOT NULL,
  steps      TEXT    NOT NULL,
  created_at INTEGER NOT NULL DEFAULT (unixepoch()),
  org_id     TEXT    NOT NULL REFERENCES organizations(id)
);
CREATE INDEX escalation_policies_org_idx ON escalation_policies (org_id);

CREATE TABLE escalation_episodes (
  id                 TEXT    PRIMARY KEY,
  monitor_id         TEXT    REFERENCES monitors(id) ON DELETE CASCADE,
  policy_id          TEXT    NOT NULL REFERENCES escalation_policies(id) ON DELETE CASCADE,
  started_at         INTEGER NOT NULL DEFAULT (unixepoch()),
  last_step          INTEGER NOT NULL DEFAULT 0,
  next_escalation_at INTEGER,
  acked_at           INTEGER,
  acked_by           TEXT    REFERENCES users(id) ON DELETE SET NULL,
  resolved_at        INTEGER,
  subject_kind       TEXT    NOT NULL DEFAULT 'monitor',
  subject_ref        TEXT    NOT NULL
);
CREATE UNIQUE INDEX escalation_episodes_open_subject_uniq
  ON escalation_episodes (subject_kind, subject_ref) WHERE resolved_at IS NULL;
