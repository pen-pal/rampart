-- Per-channel quiet hours + rate limit.
--
-- Two independent throttles, both evaluated in the notifier's dispatch
-- path *before* a send (and skipped entirely for user-initiated Test
-- events, which must always go out immediately):
--
--   quiet_hours_start / quiet_hours_end  (SMALLINT, 0-23, UTC hour)
--       When both are set, any non-Test send whose current UTC hour falls
--       inside the half-open interval [start, end) is suppressed (dropped
--       with a debug log). Wrap-around is supported: start=22, end=6 means
--       22:00-05:59 UTC is quiet. start == end is treated as "no quiet
--       window" (an empty interval) rather than "all day". NULL on either
--       column disables the quiet-hours check for the channel.
--
--   rate_limit_per_hour  (INT, NOT NULL DEFAULT 0)
--       0 = unlimited (legacy behaviour). > 0 = at most N non-Test sends
--       per rolling 1-hour window per channel; further sends inside the
--       window are dropped. The window is tracked in-memory in the
--       notifier (a per-channel timestamp ring), so it resets on restart —
--       acceptable for a soft throttle whose only job is to stop a flapping
--       monitor from hammering an expensive channel.
--
-- Quiet hours are UTC to keep the check trivial (the server already works
-- in UTC everywhere); a future per-channel timezone could layer on top.

ALTER TABLE notifications
    ADD COLUMN IF NOT EXISTS quiet_hours_start  SMALLINT NULL,
    ADD COLUMN IF NOT EXISTS quiet_hours_end    SMALLINT NULL,
    ADD COLUMN IF NOT EXISTS rate_limit_per_hour INTEGER NOT NULL DEFAULT 0;

ALTER TABLE notifications
    ADD CONSTRAINT notifications_quiet_hours_start_range
    CHECK (quiet_hours_start IS NULL OR (quiet_hours_start >= 0 AND quiet_hours_start <= 23));

ALTER TABLE notifications
    ADD CONSTRAINT notifications_quiet_hours_end_range
    CHECK (quiet_hours_end IS NULL OR (quiet_hours_end >= 0 AND quiet_hours_end <= 23));

ALTER TABLE notifications
    ADD CONSTRAINT notifications_rate_limit_per_hour_nonneg
    CHECK (rate_limit_per_hour >= 0);
