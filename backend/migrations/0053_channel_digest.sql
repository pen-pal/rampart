-- Per-channel digest / coalescing window.
--
-- When digest_window_secs > 0, the notifier buffers events destined for
-- this channel in-memory and flushes a single COMBINED message every
-- `digest_window_secs` seconds (e.g. "3 alerts in the last 60s: X down,
-- Y recovered, Z down") instead of firing each event immediately. This
-- tames flapping monitors paired with high-cost channels.
--
--   0  → off / immediate (legacy behaviour — send each event as it
--        arrives, same as before this column existed).
--   N  → coalesce into one message per N-second window, 1..=3600.
--
-- The buffer itself lives in process memory (a Mutex<HashMap<..>> in the
-- notifier service) and is intentionally NOT persisted — a coalescing
-- window measured in seconds doesn't justify durable queueing, and a
-- restart at worst drops a few seconds of buffered alerts. The DB only
-- stores the per-channel window length.
--
-- Range is validated in the DB layer (create/update clamp to 0..=3600);
-- the CHECK here is a backstop against direct writes.

ALTER TABLE notifications
    ADD COLUMN IF NOT EXISTS digest_window_secs INTEGER NOT NULL DEFAULT 0;

ALTER TABLE notifications
    ADD CONSTRAINT notifications_digest_window_secs_range
    CHECK (digest_window_secs >= 0 AND digest_window_secs <= 3600);
