-- Per-API-key rate-limit budget (item 6). Makes the previously-fixed
-- 1000/hr in-process limit configurable per key. The in-process rolling-hour
-- COUNTER is still process-local (resets on restart); only the BUDGET is
-- persisted here. Durable cross-node counters are out of scope.
ALTER TABLE api_keys
    ADD COLUMN rate_limit_per_hour INTEGER NOT NULL DEFAULT 1000;
