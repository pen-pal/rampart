-- Multi-DB P1 boot-wiring (SQLite) — append-only tamper-evident audit log.
--
-- Forked from PG 0001 (audit_log) + 0083 (prev_hash/hash chain columns). Audit
-- is GLOBAL (no org_id — it IS the cross-org security record). Dialect:
-- BIGSERIAL→INTEGER PRIMARY KEY (rowid, auto-increments, read as i64); uuid→TEXT;
-- jsonb payload→TEXT; INET ip_addr→plain TEXT (the dotted/colon address — the
-- hash + display use this verbatim, so insert and verify_chain stay
-- self-consistent); timestamptz→INTEGER unix-seconds. actor_api_key_id is
-- FK-less (api_keys not yet forked to SQLite); actor_user_id FKs the ported
-- users table.

CREATE TABLE audit_log (
  id               INTEGER PRIMARY KEY,
  actor_user_id    TEXT    REFERENCES users(id),
  actor_api_key_id TEXT,
  action           TEXT    NOT NULL,
  resource_kind    TEXT    NOT NULL,
  resource_id      TEXT,
  payload          TEXT,
  ip_addr          TEXT,
  user_agent       TEXT,
  ts               INTEGER NOT NULL DEFAULT (unixepoch()),
  prev_hash        TEXT,
  hash             TEXT
);
CREATE INDEX audit_log_ts_idx ON audit_log (ts DESC);
