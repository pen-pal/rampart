-- Multi-DB P2 (MySQL) — the status-page subsystem schema. Ported from PG (the
-- lean current model, not the v1 initial schema). Provisions the whole subsystem
-- in one migration because the domains are mutually coupled: `incidents.recent`
-- JOINs `status_pages`, while `status_pages.public_view` reads incidents. The
-- incidents domain (first consumer) ships with this migration; the status_pages
-- domain module follows in the next slice.
--
-- uuid→CHAR(36), ts→BIGINT, bool→TINYINT(1), `incident_style` enum→VARCHAR(16)
-- (serde round-trip). PG partial UNIQUE on custom_domain WHERE NOT NULL → a plain
-- UNIQUE (MySQL treats NULLs as distinct, so multiple no-domain pages coexist).

CREATE TABLE status_pages (
  id            CHAR(36)      NOT NULL PRIMARY KEY,
  slug          VARCHAR(64)   NOT NULL UNIQUE,
  title         VARCHAR(255)  NOT NULL,
  description   TEXT          NULL,
  theme         VARCHAR(16)   NOT NULL DEFAULT 'auto',
  custom_domain VARCHAR(255)  NULL UNIQUE,
  logo_url      VARCHAR(2048) NULL,
  custom_css    TEXT          NULL,
  password_hash VARCHAR(255)  NULL,
  org_id        CHAR(36)      NOT NULL,
  created_at    BIGINT        NOT NULL DEFAULT (UNIX_TIMESTAMP())
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX status_pages_org_idx ON status_pages (org_id);

CREATE TABLE status_page_sections (
  id             CHAR(36)     NOT NULL PRIMARY KEY,
  status_page_id CHAR(36)     NOT NULL,
  name           VARCHAR(255) NOT NULL,
  position       INT          NOT NULL DEFAULT 0
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX status_page_sections_page_idx ON status_page_sections (status_page_id, position);

CREATE TABLE status_page_monitors (
  page_id    CHAR(36) NOT NULL,
  monitor_id CHAR(36) NOT NULL,
  position   INT      NOT NULL DEFAULT 0,
  section_id CHAR(36) NULL,
  PRIMARY KEY (page_id, monitor_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX status_page_monitors_page_idx ON status_page_monitors (page_id, position);

CREATE TABLE incidents (
  id             CHAR(36)     NOT NULL PRIMARY KEY,
  status_page_id CHAR(36)     NOT NULL,
  title          TEXT         NOT NULL,
  content        TEXT         NOT NULL,
  style          VARCHAR(16)  NOT NULL DEFAULT 'warning',
  pinned         TINYINT(1)   NOT NULL DEFAULT 1,
  active         TINYINT(1)   NOT NULL DEFAULT 1,
  resolved_at    BIGINT       NULL,
  created_at     BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  created_by     CHAR(36)     NULL,
  dedup_key      VARCHAR(255) NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
-- PG has a partial UNIQUE (status_page_id, dedup_key) WHERE active that MySQL
-- can't express. The dedup lookup already LIMIT-1s the active match and webhook
-- ingest resolves firing incidents by find-then-update, so the constraint is a
-- belt-and-braces integrity guard, not a correctness dependency. (ponytail:
-- if a hard guarantee is needed on MySQL, add a generated `active_dedup` column
-- and UNIQUE it.)
CREATE INDEX incidents_active_idx ON incidents (status_page_id, active);

CREATE TABLE incident_updates (
  id          CHAR(36) NOT NULL PRIMARY KEY,
  incident_id CHAR(36) NOT NULL,
  message     TEXT     NOT NULL,
  posted_at   BIGINT   NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  posted_by   CHAR(36) NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX incident_updates_idx ON incident_updates (incident_id, posted_at);
