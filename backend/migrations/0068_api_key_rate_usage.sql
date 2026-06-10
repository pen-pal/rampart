-- Durable per-API-key rate-limit counter (item 6).
--
-- The per-key BUDGET (api_keys.rate_limit_per_hour, migration 0067) was
-- already persisted, but the COUNTER lived in an in-process rolling-hour
-- deque that reset on every restart. This table makes the counter durable
-- across restarts.
--
-- A FIXED-window counter: one row per key holding the start of the current
-- window and the number of requests admitted in it. Simpler and durable;
-- the rolling-window precision loss (a burst can straddle the boundary and
-- briefly exceed the budget within any trailing hour) is acceptable for what
-- is a courtesy throttle and an advisory header set, not a hard cross-node
-- quota.
CREATE TABLE api_key_rate_usage (
    api_key_id   UUID PRIMARY KEY REFERENCES api_keys(id) ON DELETE CASCADE,
    window_start TIMESTAMPTZ NOT NULL,
    count        INTEGER NOT NULL
);
