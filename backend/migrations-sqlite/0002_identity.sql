-- Identity + tenancy core (users / organizations / org_members), forked from the
-- Postgres schema (migrations/0001_initial.sql + 0107_organizations.sql + the
-- role/totp/prefs ALTERs, flattened to the current effective shape).
--
-- SQLite dialect mapping:
--   uuid           -> TEXT  (hyphenated string)
--   citext         -> TEXT  UNIQUE COLLATE NOCASE (case-insensitive email)
--   user_role enum -> TEXT  CHECK (admin|editor|readonly)
--   jsonb          -> TEXT  (JSON)
--   timestamptz    -> INTEGER unix-seconds (unixepoch(); clean i64<->OffsetDateTime)
--   boolean        -> INTEGER 0/1
--   regex CHECK    -> GLOB negated-class (no native regex)
CREATE TABLE users (
  id                    TEXT    PRIMARY KEY,
  email                 TEXT    NOT NULL UNIQUE COLLATE NOCASE,
  name                  TEXT,
  password_hash         TEXT    NOT NULL,
  totp_secret           TEXT,
  is_admin              INTEGER NOT NULL DEFAULT 0,
  created_at            INTEGER NOT NULL DEFAULT (unixepoch()),
  last_login_at         INTEGER,
  totp_enabled          INTEGER NOT NULL DEFAULT 0,
  role                  TEXT    NOT NULL DEFAULT 'editor' CHECK (role IN ('admin','editor','readonly')),
  prefs                 TEXT    NOT NULL DEFAULT '{}',
  totp_failed_attempts  INTEGER NOT NULL DEFAULT 0,
  totp_locked_until     INTEGER
);

CREATE TABLE organizations (
  id          TEXT    PRIMARY KEY,
  slug        TEXT    NOT NULL UNIQUE
                CHECK (length(slug) BETWEEN 2 AND 40 AND slug NOT GLOB '*[^a-z0-9-]*'),
  name        TEXT    NOT NULL,
  created_at  INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE org_members (
  org_id      TEXT    NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
  user_id     TEXT    NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  role        TEXT    NOT NULL CHECK (role IN ('admin','editor','readonly')),
  created_at  INTEGER NOT NULL DEFAULT (unixepoch()),
  PRIMARY KEY (org_id, user_id)
);

-- The Default org (migration 0107 seeds this on Postgres; mirror it here so a
-- fresh SQLite instance is single-org-functional out of the box).
INSERT INTO organizations (id, slug, name)
VALUES ('00000000-0000-0000-0000-000000000001', 'default', 'Default');
