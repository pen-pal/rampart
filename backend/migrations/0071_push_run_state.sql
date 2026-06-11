-- Cron-job monitoring on push monitors (Cronitor-style).
--
-- A push monitor can now receive three ping STATES instead of a bare
-- liveness ping: `run` (job started), `complete` (job finished OK), and
-- `fail` (job finished broken). `last_run_started_at` holds the open
-- run's start so the completion ping can compute the job's duration
-- (recorded as the heartbeat's latency_ms) and so the scheduler can flag
-- runs that exceed `config.max_run_seconds` while still in flight.
-- Cleared on complete/fail. NULL for non-push monitors and for push
-- monitors that only ever send bare pings.
--
-- The schedule itself (`config.cron`, `config.cron_grace_seconds`,
-- `config.max_run_seconds`) lives in the existing config JSONB — no
-- schema needed there.
ALTER TABLE monitors
    ADD COLUMN last_run_started_at TIMESTAMPTZ NULL;
