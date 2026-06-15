-- Service Level Objectives (SLOs) with error budgets — a first-class tier.
--
-- Distinct from the per-monitor `slo_target_pct` field on `monitors`, which is
-- a simple "you promised X% uptime, you're at Y%" marker. An SLO row here is a
-- named objective with a rolling error budget and burn-rate alerting, over one
-- of two indicator kinds:
--
--   * monitor — availability = up heartbeats / total heartbeats in the window.
--   * metric  — ratio = SUM(good_metric) / SUM(total_metric) over the window,
--               for ratio-of-counters SLIs (e.g. successful / total requests).
--
-- Evaluation (scheduler) computes the achieved ratio over the window, derives
-- the consumed error budget, and additionally checks a short-window fast-burn
-- (Google SRE style). `breaching_at` de-dups the page across ticks and drives
-- the fire/resolve state machine, optionally climbing an escalation policy.

CREATE TABLE slos (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name          TEXT             NOT NULL,
    description   TEXT             NOT NULL DEFAULT '',
    sli_kind      TEXT             NOT NULL CHECK (sli_kind IN ('monitor', 'metric')),

    -- monitor SLI
    monitor_id    UUID             REFERENCES monitors(id) ON DELETE CASCADE,

    -- metric SLI: ratio of two summed counters over the window
    good_metric   TEXT,
    total_metric  TEXT,
    labels        JSONB            NOT NULL DEFAULT '{}'::jsonb,

    objective_pct DOUBLE PRECISION NOT NULL CHECK (objective_pct > 0 AND objective_pct < 100),
    window_days   INT              NOT NULL DEFAULT 30 CHECK (window_days BETWEEN 1 AND 365),

    enabled       BOOLEAN          NOT NULL DEFAULT TRUE,
    channel_ids   UUID[]           NOT NULL DEFAULT '{}',
    escalation_policy_id UUID       REFERENCES escalation_policies(id) ON DELETE SET NULL,

    -- set the first tick the budget is exhausted / fast-burning; cleared on
    -- recovery. NULL means "not currently breaching".
    breaching_at  TIMESTAMPTZ,

    created_at    TIMESTAMPTZ      NOT NULL DEFAULT now(),

    CONSTRAINT slos_monitor_present CHECK (sli_kind <> 'monitor' OR monitor_id IS NOT NULL),
    CONSTRAINT slos_metric_present  CHECK (sli_kind <> 'metric'  OR (good_metric IS NOT NULL AND total_metric IS NOT NULL))
);

CREATE INDEX slos_enabled_idx ON slos (enabled) WHERE enabled;
