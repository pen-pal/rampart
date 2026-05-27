-- TLS certificate snapshot for HTTP monitors.
--
-- Populated by the scheduler when an http(s) monitor's probe runs.
-- Refreshed at most once per hour per monitor so HTTPS sites with
-- 30-second intervals don't spend the bulk of every tick re-parsing
-- the same cert chain.

ALTER TABLE monitors
    ADD COLUMN IF NOT EXISTS cert_days_left   INT,
    ADD COLUMN IF NOT EXISTS cert_subject     TEXT,
    ADD COLUMN IF NOT EXISTS cert_checked_at  TIMESTAMPTZ;
