-- Multi-tenancy Phase 5: per-org ingest credentials.
--
-- Generalizes the per-status-page `ingest_tokens` to per-ORG keys. A telemetry
-- client (OTLP / Prometheus remote_write / RUM / profiles) presents the key in
-- the same Bearer / X-Rampart-Token / ?k slot the global telemetry_token uses
-- today, and the ingest path resolves the owning org from it (see
-- `resolve_ingest_org`). With NO key minted, ingest falls back to the legacy
-- global-token gate and lands on the Default org — so single-org / existing
-- installs are behaviour-identical until an operator mints org-scoped keys.
--
-- `allowed_origins` (P5-3) origin-binds RUM keys: the RUM token necessarily
-- ships in the browser snippet, so for multi-org installs it is checked against
-- the request Origin to stop a forged key misattributing beacons cross-org.
CREATE TABLE ingest_keys (
    id              UUID        PRIMARY KEY,
    org_id          UUID        NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    token           TEXT        NOT NULL UNIQUE,
    label           TEXT        NOT NULL DEFAULT '',
    -- which ingest tier(s) the key is for; 'all' accepts any. Advisory for now.
    kind            TEXT        NOT NULL DEFAULT 'all',
    -- NULL = no origin restriction; non-null = allowed Origin header values.
    allowed_origins TEXT[]      NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at    TIMESTAMPTZ NULL
);

CREATE INDEX ingest_keys_org_idx ON ingest_keys (org_id);
