-- Reusable monitor config presets.
--
-- Operators repeatedly type the same handful of HTTP request headers
-- (Authorization bearer skeletons, X-Api-Version, a fixed User-Agent…)
-- or the same TLS posture (ignore_tls for a self-signed internal box)
-- across many monitors. A preset is a named, reusable bag of one of
-- those config slices the New-Monitor wizard can apply with a click.
--
-- `kind` discriminates what `data` carries:
--   'http_headers' → { headers: { "Name": "value", … } }
--   'tls'          → { ignore_tls: bool }
-- The shape is intentionally loose JSONB so future preset kinds (or
-- extra TLS knobs) don't need a migration — the wizard owns the schema.
--
-- Global, not monitor-scoped: the whole point is reuse across monitors.

CREATE TABLE monitor_presets (
    id         UUID PRIMARY KEY,
    name       TEXT        NOT NULL,
    kind       TEXT        NOT NULL CHECK (kind IN ('http_headers', 'tls')),
    data       JSONB       NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
