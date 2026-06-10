-- Notification delivery log — an append-only record of every channel
-- send attempt the notifier makes (success OR failure).
--
-- Background: the notifier dispatches each event to its routed channels
-- best-effort and fire-and-forget; failures were only ever visible in the
-- process logs. This table gives operators a durable, queryable history of
-- what actually went out (and what failed), surfaced via GET /v1/delivery-log
-- and a future dashboard view.
--
-- One row per channel send attempt. Recording is best-effort in the
-- notifier — a failure to write a delivery_log row must never break the
-- dispatch path — so this table is purely observational.
--
--   notification_id  the channel the attempt targeted. FK to notifications
--                    with ON DELETE SET NULL: deleting a channel keeps its
--                    historical delivery rows (the log outlives the channel)
--                    but drops the dangling reference.
--   channel_kind     the channel kind string (e.g. "slack", "email") — kept
--                    denormalised so the log is readable after the channel
--                    row is gone.
--   event_kind       the EventKind that triggered the send (e.g.
--                    "status_flip", "maintenance_started").
--   monitor_id       the monitor the event was about, when applicable.
--   ok               whether the send succeeded.
--   error            the failure detail when ok = false; NULL on success.
--   sent_at          when the attempt completed.

CREATE TABLE IF NOT EXISTS delivery_log (
    id              BIGSERIAL PRIMARY KEY,
    notification_id UUID REFERENCES notifications(id) ON DELETE SET NULL,
    channel_kind    TEXT NOT NULL,
    event_kind      TEXT NOT NULL,
    monitor_id      UUID,
    ok              BOOLEAN NOT NULL,
    error           TEXT,
    sent_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_delivery_log_sent_at
    ON delivery_log (sent_at DESC);
