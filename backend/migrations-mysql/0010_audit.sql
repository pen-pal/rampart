-- Multi-DB P2 (MySQL) — append-only, tamper-evident audit log. Forked from
-- PG/SQLite. BIGSERIAL→BIGINT AUTO_INCREMENT, INET→VARCHAR (plain IpAddr text),
-- ts→BIGINT, sha256-hex hashes→VARCHAR(128), payload jsonb→LONGTEXT.
--
-- Chain serialization: PG uses pg_advisory_xact_lock; SQLite relies on its
-- single-writer transaction. MySQL InnoDB has neither, and GET_LOCK is
-- session-scoped (leaks on a pooled conn). So a dedicated single-row
-- `audit_chain_lock` is locked `FOR UPDATE` inside the insert transaction —
-- tx-scoped (auto-released on commit, no leak) and always present (covers the
-- genesis insert), giving a strictly linear append order.

CREATE TABLE audit_log (
  id               BIGINT       NOT NULL AUTO_INCREMENT PRIMARY KEY,
  actor_user_id    CHAR(36)     REFERENCES users(id),
  actor_api_key_id CHAR(36),
  action           VARCHAR(128) NOT NULL,
  resource_kind    VARCHAR(64)  NOT NULL,
  resource_id      CHAR(36),
  payload          LONGTEXT,
  ip_addr          VARCHAR(64),
  user_agent       TEXT,
  ts               BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  prev_hash        VARCHAR(128),
  hash             VARCHAR(128)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX audit_log_ts_idx ON audit_log (ts);

CREATE TABLE audit_chain_lock (k TINYINT NOT NULL PRIMARY KEY) ENGINE=InnoDB;
INSERT INTO audit_chain_lock (k) VALUES (1);
