-- Per-monitor SLO targets.
--
-- A simple "you promised 99.9% — current rolling uptime is 99.85%" check.
-- Not a full SLI/SLO error-budget calculator: just a target percentage,
-- a rolling window in days, and (later) a single de-dup column to
-- remember that we already alerted on the current breach.
--
-- Both columns are nullable. NULL slo_target_pct means "no SLO configured
-- for this monitor" — the breach-detection helper short-circuits in that
-- case so monitors without an SLO carry zero extra cost per heartbeat.

ALTER TABLE monitors
    ADD COLUMN IF NOT EXISTS slo_target_pct  NUMERIC(5,3)
        CHECK (slo_target_pct IS NULL
               OR (slo_target_pct >= 90.0 AND slo_target_pct <= 100.0)),
    ADD COLUMN IF NOT EXISTS slo_window_days INT
        DEFAULT 30
        CHECK (slo_window_days BETWEEN 1 AND 90);
