-- Multi-DB P2 (MySQL) — identity + tenancy core (users / organizations /
-- org_members), forked from the PG schema (0001_initial + 0107_organizations +
-- role/totp/prefs ALTERs) via the SQLite 0002_identity shape.
--
-- MySQL dialect mapping:
--   uuid        -> CHAR(36) (hyphenated string)
--   citext      -> VARCHAR + utf8mb4_0900_ai_ci (case-insensitive UNIQUE email)
--   user_role   -> VARCHAR(16) + CHECK (admin|editor|readonly)
--   jsonb       -> LONGTEXT (JSON text; no JSON_EXTRACT needed on these cols)
--   timestamptz -> BIGINT unix-seconds (DEFAULT (UNIX_TIMESTAMP()), 8.0.13+ expr default)
--   boolean     -> TINYINT 0/1
--   regex CHECK -> REGEXP (MySQL 8 CHECK supports it)

CREATE TABLE users (
  id                   CHAR(36)     NOT NULL PRIMARY KEY,
  email                VARCHAR(320) NOT NULL UNIQUE,
  name                 VARCHAR(255),
  password_hash        TEXT         NOT NULL,
  totp_secret          TEXT,
  is_admin             TINYINT      NOT NULL DEFAULT 0,
  created_at           BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  last_login_at        BIGINT,
  totp_enabled         TINYINT      NOT NULL DEFAULT 0,
  role                 VARCHAR(16)  NOT NULL DEFAULT 'editor'
                         CHECK (role IN ('admin', 'editor', 'readonly')),
  prefs                LONGTEXT     NOT NULL DEFAULT ('{}'),
  totp_failed_attempts INT          NOT NULL DEFAULT 0,
  totp_locked_until    BIGINT
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE organizations (
  id         CHAR(36)     NOT NULL PRIMARY KEY,
  slug       VARCHAR(40)  NOT NULL UNIQUE
               CHECK (CHAR_LENGTH(slug) BETWEEN 2 AND 40 AND slug REGEXP '^[a-z0-9-]+$'),
  name       VARCHAR(255) NOT NULL,
  created_at BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP())
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE org_members (
  org_id     CHAR(36)    NOT NULL,
  user_id    CHAR(36)    NOT NULL,
  role       VARCHAR(16) NOT NULL CHECK (role IN ('admin', 'editor', 'readonly')),
  created_at BIGINT      NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  PRIMARY KEY (org_id, user_id),
  FOREIGN KEY (org_id) REFERENCES organizations(id) ON DELETE CASCADE,
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- The Default org (PG 0107 seeds it; mirror so a fresh MySQL instance is
-- single-org-functional out of the box).
INSERT INTO organizations (id, slug, name)
VALUES ('00000000-0000-0000-0000-000000000001', 'default', 'Default');
