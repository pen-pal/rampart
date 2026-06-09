-- Per-monitor SLO breach de-duplication marker.
--
-- The scheduler's writer loop now runs a per-monitor SLO check after every
-- persisted heartbeat batch. We need a single column to remember whether
-- we already paged for the *current* breach so a sustained dip doesn't
-- spam every channel on every heartbeat.
--
-- Semantics:
--   slo_breached_at IS NULL  → no active breach. If current uptime drops
--                              below the target, emit SloBreached and
--                              stamp NOW().
--   slo_breached_at IS NOT NULL → breach already announced. If current
--                                 uptime climbs back at-or-above the
--                                 target, emit SloRecovered and clear
--                                 the column. While still below target,
--                                 stay silent.
--
-- Only one column on purpose — this is a heads-up, not a full SLO/error
-- budget engine. See migration 0044 for the target + window settings.

ALTER TABLE monitors
    ADD COLUMN IF NOT EXISTS slo_breached_at TIMESTAMPTZ NULL;
