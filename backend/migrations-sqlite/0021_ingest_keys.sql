-- Multi-DB P1 domain-port (SQLite) — per-org ingest credentials (multi-tenancy
-- Phase 5). Forked from PG `ingest_keys` + its token_hash follow-up. A telemetry
-- client (OTLP / Prometheus remote_write / RUM / profiles) presents the key in
-- the Bearer / X-Rampart-Token / ?k slot; the ingest path resolves the owning
-- org from it. Dialect: uuid→TEXT, ts→INTEGER unix-seconds, TEXT[] allowed
-- origins→JSON TEXT. Dual token + token_hash columns mirror the PG hash-primary
-- lookup with a plaintext fallback (both UNIQUE).

CREATE TABLE ingest_keys (
  id              TEXT    PRIMARY KEY,
  org_id          TEXT    NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
  token           TEXT    NOT NULL UNIQUE,
  token_hash      TEXT    UNIQUE,
  label           TEXT    NOT NULL DEFAULT '',
  -- which ingest tier(s) the key is for; 'all' accepts any surface.
  kind            TEXT    NOT NULL DEFAULT 'all',
  -- NULL = no origin restriction; non-null = JSON array of allowed Origin values.
  allowed_origins TEXT,
  created_at      INTEGER NOT NULL DEFAULT (unixepoch()),
  last_used_at    INTEGER
);
CREATE INDEX ingest_keys_org_idx ON ingest_keys (org_id);
