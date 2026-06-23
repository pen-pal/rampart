-- Multi-DB P2 (MySQL/MariaDB) — the P0 spike table that proves the toolchain
-- (sqlx mysql driver + `#[sqlx::test]` fixture + the ON DUPLICATE KEY upsert
-- dialect). Mirrors the PG/SQLite `settings` key/value store.
--
-- Dialect vs PG: `key` is a reserved word → backticked; uuid→CHAR(36) and
-- timestamptz→BIGINT (unix seconds) conventions apply to later domains; JSON
-- values are stored as LONGTEXT here (settings never queries INTO the value, so
-- the native JSON type buys nothing — value-querying domains like slos/detection
-- will use JSON + JSON_EXTRACT). VARCHAR(190) PK stays under the utf8mb4 index
-- key-length limit.

CREATE TABLE settings (
  `key`      VARCHAR(190) NOT NULL PRIMARY KEY,
  value      LONGTEXT     NOT NULL,
  updated_at TIMESTAMP    NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
