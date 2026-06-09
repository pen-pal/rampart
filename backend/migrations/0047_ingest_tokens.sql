-- Inbound ingest tokens — page-scoped webhook credentials.
--
-- A token is an opaque random string embedded in the public ingest URL
-- (e.g. Prometheus Alertmanager's webhook_config `url`). It is NOT a user
-- session: it authorizes one external system to post alerts that become
-- status-page incidents on exactly one status page.
--
-- These creds are page-scoped and intentionally low-privilege: the only
-- thing they can do is create / resolve incidents on their target page.
-- They are shown to the admin in plaintext (unlike personal API keys)
-- because the operator has to paste the full value into the alerting
-- system's config and there is no way to re-derive it otherwise.

CREATE TABLE ingest_tokens (
    id UUID PRIMARY KEY,
    status_page_id UUID NOT NULL REFERENCES status_pages(id) ON DELETE CASCADE,
    token TEXT NOT NULL UNIQUE,        -- opaque random string, the URL credential
    label TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at TIMESTAMPTZ
);
CREATE INDEX ON ingest_tokens(status_page_id);
