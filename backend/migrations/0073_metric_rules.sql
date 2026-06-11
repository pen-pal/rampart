-- Threshold alert rules over ingested metrics (observability tier).
--
-- A rule watches ONE series (metric name + exact label set) and compares
-- its latest sample against a threshold. `for_seconds` is the sustain
-- window: the breach must hold continuously that long before the rule
-- fires (0 = fire on first breach). Pending/firing state is persisted on
-- the row so evaluation survives restarts and never double-pages:
--
--   breach_since — when the current uninterrupted breach started; set on
--                  the first breached evaluation, cleared when a sample
--                  comes back inside the threshold.
--   firing_at    — non-NULL while the rule is actively firing (breach
--                  outlasted for_seconds and the alert went out). Cleared
--                  on resolve, which is when the recovery notice sends.
--
-- `channel_ids` holds notification-channel UUIDs directly (no join
-- table): rules are few, the array is small, and channels deleted later
-- are skipped at send time. Monitors' tag-routing doesn't apply — rules
-- aren't monitors.
CREATE TABLE metric_rules (
    id           UUID PRIMARY KEY,
    name         TEXT             NOT NULL,
    metric       TEXT             NOT NULL,
    labels       JSONB            NOT NULL DEFAULT '{}',
    op           TEXT             NOT NULL CHECK (op IN ('gt', 'lt', 'gte', 'lte')),
    threshold    DOUBLE PRECISION NOT NULL,
    for_seconds  INTEGER          NOT NULL DEFAULT 0 CHECK (for_seconds >= 0),
    enabled      BOOLEAN          NOT NULL DEFAULT TRUE,
    channel_ids  UUID[]           NOT NULL DEFAULT '{}',
    breach_since TIMESTAMPTZ      NULL,
    firing_at    TIMESTAMPTZ      NULL,
    created_at   TIMESTAMPTZ      NOT NULL DEFAULT now()
);
