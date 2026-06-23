-- Multi-DB P1 (SQLite) — remote probe agents.
--
-- Forked from PG migrations 0070 (agents + monitors.agent_id, already a column
-- in migrations-sqlite/0004_monitors.sql) and 0108 (org_id). Dialect map:
-- uuid → TEXT, timestamptz → INTEGER unix-seconds (`unixepoch()`), the
-- token_hash UNIQUE survives verbatim (the lookup hot path).
--
-- monitors.agent_id already exists (declared in 0004 as a plain TEXT column —
-- no FK, since `agents` is created here, after `monitors`). The PG schema's
-- `ON DELETE SET NULL` is therefore not enforced on SQLite yet; `delete()`
-- below clears the column explicitly to keep the same behaviour (orphaned
-- monitors fall back to local probing).

CREATE TABLE agents (
    id           TEXT    PRIMARY KEY,
    name         TEXT    NOT NULL,
    location     TEXT,
    token_hash   TEXT    NOT NULL UNIQUE,
    version      TEXT,
    last_seen_at INTEGER,
    created_at   INTEGER NOT NULL DEFAULT (unixepoch()),
    org_id       TEXT    NOT NULL DEFAULT '00000000-0000-0000-0000-000000000001'
                         REFERENCES organizations(id)
);

CREATE INDEX agents_org_idx ON agents (org_id);
