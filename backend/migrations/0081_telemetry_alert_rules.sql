-- Threshold alert rules over the observability tiers (errors / traces / logs).
--
-- A sibling of `metric_rules` (0073): same persisted state-machine columns
-- (breach_since / firing_at) and the same `for_seconds` sustain window, so it
-- reuses `rampart_core::metric_rule::rule_transition` verbatim. The difference
-- is the observed value — instead of "latest metric sample" it's a windowed
-- aggregate over one tier, chosen by `kind`:
--
--   error_rate       — COUNT(error_events) in the window (optionally one
--                      project, matched by `target` = project name).
--   trace_latency    — p95 span duration_ms in the window (optionally one
--                      service, matched by `target` = service_name).
--   trace_error_rate — percent of spans with status_code = 2 (error) in the
--                      window, same optional service scope.
--   log_volume       — COUNT(logs) in the window at or above `min_level`
--                      (OTLP severity number), optionally one service
--                      (`target`) and/or a body substring (`match_text`).
--
-- `target` = '' means "all" (no scope filter). `op`/`threshold` compare the
-- aggregate the same way metric rules do. `channel_ids` holds notification
-- channel UUIDs directly (rules aren't monitors; no tag routing).
CREATE TABLE telemetry_alert_rules (
    id             UUID             PRIMARY KEY,
    name           TEXT             NOT NULL,
    kind           TEXT             NOT NULL CHECK (kind IN (
                       'error_rate', 'trace_latency', 'trace_error_rate', 'log_volume')),
    target         TEXT             NOT NULL DEFAULT '',
    match_text     TEXT             NOT NULL DEFAULT '',
    min_level      SMALLINT         NOT NULL DEFAULT 0,
    op             TEXT             NOT NULL CHECK (op IN ('gt', 'lt', 'gte', 'lte')),
    threshold      DOUBLE PRECISION NOT NULL,
    window_seconds INTEGER          NOT NULL DEFAULT 300 CHECK (window_seconds > 0),
    for_seconds    INTEGER          NOT NULL DEFAULT 0 CHECK (for_seconds >= 0),
    enabled        BOOLEAN          NOT NULL DEFAULT TRUE,
    channel_ids    UUID[]           NOT NULL DEFAULT '{}',
    breach_since   TIMESTAMPTZ      NULL,
    firing_at      TIMESTAMPTZ      NULL,
    created_at     TIMESTAMPTZ      NOT NULL DEFAULT now()
);
