-- Durable backing store for the per-channel notification digest buffer.
--
-- Background: when a channel has digest_window_secs > 0 the notifier
-- coalesces events into one combined message per window (see migration
-- 0053). Originally that buffer lived only in process memory, so a restart
-- dropped every pending-but-not-yet-flushed event. This table makes the
-- buffer durable: each buffered event is persisted here on enqueue and
-- deleted once its window flushes. On boot the flush task picks up any
-- rows left behind by the previous process and resumes them.
--
-- One row per buffered event (NOT one per channel) — the combined message
-- is rendered at flush time from all rows sharing a notification_id.
--
--   notification_id  the channel the event is buffered for. FK to
--                    notifications with ON DELETE CASCADE so deleting a
--                    channel discards its pending buffer.
--   event_json       the serialized rampart-notifier `Event` (JSONB).
--   enqueued_at      when the event landed. MIN(enqueued_at) per channel
--                    is the window start used to decide when to flush.

CREATE TABLE IF NOT EXISTS digest_buffer (
    id              UUID PRIMARY KEY,
    notification_id UUID NOT NULL REFERENCES notifications(id) ON DELETE CASCADE,
    event_json      JSONB NOT NULL,
    enqueued_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_digest_buffer_notification_id
    ON digest_buffer (notification_id);
