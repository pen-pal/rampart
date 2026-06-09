-- Generic JSON-path webhook mapping for ingest tokens.
--
-- The five named receivers (alertmanager / grafana / datadog / pagerduty /
-- opsgenie) hard-code the shape of their vendor's payload. The "generic"
-- receiver instead lets the operator describe, per token, how to pull the
-- normalized incident fields out of an arbitrary inbound JSON body using
-- RFC 6901 JSON Pointers (the same syntax serde_json::Value::pointer()
-- understands natively — no new dependency).
--
-- The column is NULLABLE: only tokens used with the generic receiver need a
-- mapping. The named receivers ignore it entirely. A token with no mapping
-- POSTed to the generic receiver is a 400 (clear "no mapping configured").
--
-- Mapping shape (all fields optional; absent pointers degrade gracefully):
--   {
--     "action_path":           "/status",     -- JSON Pointer to the firing/resolved discriminator
--     "action_firing_value":   "firing",      -- value at action_path meaning "create"
--     "action_resolved_value": "resolved",    -- value at action_path meaning "resolve"
--     "title_path":            "/labels/alertname",
--     "content_path":          "/annotations/summary",
--     "dedup_path":            "/fingerprint",
--     "style":                 "warning"       -- default incident style (info|warning|danger|primary|success)
--   }
--
-- Semantics (implemented in routes/ingest.rs):
--   - action: if the value at action_path == action_firing_value  -> Create
--             if it == action_resolved_value                       -> Resolve
--             otherwise a present alert is treated as Create (firing),
--             so a misconfigured/absent discriminator never drops the alert.
--   - title:   string at title_path; falls back to dedup key then content.
--   - content: string at content_path; defaults to empty.
--   - dedup:   string at dedup_path; falls back to the title.
--   - style:   mapping.style, defaulting to "info".

ALTER TABLE ingest_tokens
    ADD COLUMN mapping JSONB;

COMMENT ON COLUMN ingest_tokens.mapping IS
    'Optional RFC 6901 JSON-Pointer mapping for the generic webhook receiver; NULL for tokens used only with the named vendor receivers.';
