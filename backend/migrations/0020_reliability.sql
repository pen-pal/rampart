-- Reliability gaps:
--   1. Heartbeats grow unbounded — add a retention setting key.
--   2. Notification cooldowns to suppress flap storms (per-channel).
--   3. Webhook HMAC signing — covered by a new `secret` config field
--      on the existing notifications row (no schema change needed; the
--      Generic Webhook adapter just reads it out of `config`).

ALTER TABLE notifications
    ADD COLUMN IF NOT EXISTS cooldown_seconds INT NOT NULL DEFAULT 0;

-- Seed the retention setting if it's not there yet. 90 days is the
-- same window the dashboard's uptime strip rolls up.
INSERT INTO settings (key, value)
VALUES ('retention_days', '{"heartbeats": 90, "audit_log": 365}'::jsonb)
ON CONFLICT (key) DO NOTHING;
