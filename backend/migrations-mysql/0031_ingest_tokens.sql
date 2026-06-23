-- Multi-DB P2 (MySQL) — page-scoped inbound webhook ingest tokens. Ported from
-- PG. Dual-write token + token_hash (hash-primary lookup with plaintext
-- fallback, same as ingest_keys). uuid→CHAR(36), ts→BIGINT, mapping jsonb→
-- LONGTEXT. Org tenanting flows through the owning status page.

CREATE TABLE ingest_tokens (
  id             CHAR(36)     NOT NULL PRIMARY KEY,
  status_page_id CHAR(36)     NOT NULL,
  token          VARCHAR(64)  NOT NULL UNIQUE,
  token_hash     VARCHAR(128) NOT NULL UNIQUE,
  label          VARCHAR(255) NULL,
  mapping        LONGTEXT     NULL,
  created_at     BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  last_used_at   BIGINT       NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX ingest_tokens_page_idx ON ingest_tokens (status_page_id);
