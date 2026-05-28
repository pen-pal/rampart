-- Maintenance recurrence.
--
-- `start_at` / `end_at` continue to define the first occurrence; the
-- new `recurrence` JSONB captures how that template repeats. Shapes:
--   { "kind": "none" }                               -- single-shot
--   { "kind": "daily",  "until": "2026-12-31T..."? } -- every day
--   { "kind": "weekly", "weekdays": [0..6],
--                       "until":   "..."?           } -- 0 = Sunday
--
-- Recurrence eval happens in Rust against the small windows table (one
-- pass per scheduler tick on the monitor's own windows). We index on
-- `active` so the eval scan stays cheap.

ALTER TABLE maintenance_windows
    ADD COLUMN IF NOT EXISTS recurrence JSONB NOT NULL DEFAULT '{"kind":"none"}'::jsonb;

-- Companion index for the recurrence-eval path: pick all active windows
-- attached to a monitor, then walk in Rust. Two-key index covers both
-- the JOIN and the WHERE clause.
CREATE INDEX IF NOT EXISTS maintenance_windows_active_idx
    ON maintenance_windows (active);
