-- Reusable monitor templates.
--
-- Distinct from monitor_presets (which are small config slices — a saved
-- header set, a TLS posture — the wizard applies to a monitor): a template
-- is a WHOLE reusable monitor spec. An operator saves the full creation
-- body (kind + url/hostname + interval + timeouts + HTTP knobs …) under a
-- name, then instantiates it into a brand-new monitor with one call,
-- optionally overriding the name.
--
-- `spec` is the freeform monitor-creation body (a `NewMonitor` JSON object)
-- stored verbatim as JSONB. Keeping it loose means new monitor fields never
-- require a migration here — the spec is fed straight back through the same
-- monitor-create path on instantiate.
--
-- Global, not monitor-scoped: the whole point is reuse across monitors.

CREATE TABLE monitor_templates (
    id          UUID PRIMARY KEY,
    name        TEXT        NOT NULL,
    description TEXT        NULL,
    spec        JSONB       NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
