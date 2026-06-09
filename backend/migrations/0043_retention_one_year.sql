-- Bump default heartbeat retention 90 → 365 days.
--
-- Motivation: production-grade public status pages now ship a 12-month
-- summary row under the 90-day daily strip ("Jun 99.97% · Jul 100% ·
-- …"). The summary query is a per-monitor `GROUP BY date_trunc('month',
-- ts)`, so it needs heartbeats going back 12 months in the table; the
-- old 90-day prune cap meant the summary would only ever have ~3
-- months of data.
--
-- Behaviour on existing installs:
--   - The row already in `settings` is left alone if it carries an
--     operator-tuned value (their explicit choice wins).
--   - If the row is missing OR still holds the original 90-day seed,
--     it's promoted to 365 so historical data starts accumulating
--     from the first scheduled prune after this migration runs.
--   - No data is deleted: the prune loop only ever removes rows older
--     than `heartbeats` days, never adds them. Operators wanting to
--     keep the old 90-day window can write
--       UPDATE settings SET value = '{"heartbeats":90,"audit_log":365}'::jsonb
--       WHERE key = 'retention_days';
--     after the migration.

INSERT INTO settings (key, value)
VALUES ('retention_days', '{"heartbeats": 365, "audit_log": 365}'::jsonb)
ON CONFLICT (key) DO UPDATE
   SET value = EXCLUDED.value
   WHERE settings.value = '{"heartbeats": 90, "audit_log": 365}'::jsonb;
