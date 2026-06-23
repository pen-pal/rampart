-- Multi-DB P2 (MySQL) management-API tail — API keys + per-org ingest keys.
-- Ported from PG. uuid→CHAR(36), ts→BIGINT, TEXT[]→LONGTEXT(JSON). The legacy
-- `scopes` array (a deprecated PG rollback shim) is dropped — `scope` is
-- authoritative. ingest_keys keeps both token + token_hash (dual-write) so the
-- hash-primary lookup with plaintext fallback works.

CREATE TABLE api_keys (
  id                  CHAR(36)     NOT NULL PRIMARY KEY,
  name                VARCHAR(255) NOT NULL,
  key_hash            VARCHAR(128) NOT NULL UNIQUE,
  key_prefix          VARCHAR(32)  NOT NULL,
  scope               VARCHAR(32)  NOT NULL,
  created_by          CHAR(36),
  created_at          BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  last_used_at        BIGINT,
  expires_at          BIGINT,
  rate_limit_per_hour INT          NOT NULL DEFAULT 0,
  org_id              CHAR(36)     NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX api_keys_org_idx ON api_keys (org_id);

CREATE TABLE ingest_keys (
  id              CHAR(36)     NOT NULL PRIMARY KEY,
  org_id          CHAR(36)     NOT NULL,
  token           VARCHAR(64)  NOT NULL UNIQUE,
  token_hash      VARCHAR(128) NOT NULL UNIQUE,
  label           VARCHAR(255) NOT NULL,
  kind            VARCHAR(32)  NOT NULL,
  allowed_origins LONGTEXT,
  created_at      BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  last_used_at    BIGINT
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX ingest_keys_org_idx ON ingest_keys (org_id);
