-- Maintenance start/end notification de-duplication markers.
--
-- The scheduler runs a periodic best-effort scan (next to the slow
-- reconcile tick) that detects when a maintenance window transitions
-- into-active (its start instant just passed) or out-of-active (its end
-- instant just passed) and fires MaintenanceStarted / MaintenanceEnded
-- notifications to the channels attached to the window's monitors plus
-- any status-page email subscribers.
--
-- These two columns are the single-column de-dup state machine — same
-- pattern as monitors.slo_breached_at (migration 0045):
--
--   notified_start_at IS NULL  → start notification not yet sent. When
--                                the window becomes active, fire
--                                MaintenanceStarted and stamp NOW().
--   notified_end_at   IS NULL  → end notification not yet sent. When the
--                                window leaves its active span, fire
--                                MaintenanceEnded and stamp NOW().
--
-- Stamping guarantees each transition pages exactly once even though the
-- scan re-runs every slow tick.

ALTER TABLE maintenance_windows
    ADD COLUMN IF NOT EXISTS notified_start_at TIMESTAMPTZ NULL,
    ADD COLUMN IF NOT EXISTS notified_end_at   TIMESTAMPTZ NULL;
