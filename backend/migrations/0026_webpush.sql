-- Web Push (RFC 8291) channel.
--
-- A `webpush` notification row is a named target; browsers subscribe to
-- it and their subscription (endpoint + keys) is stored here. On a fire,
-- the channel fans out an encrypted push to every live subscription.
--
-- The VAPID keypair is shared across all webpush channels and lives in
-- the settings table (key 'webpush_vapid'), generated on first use.

ALTER TYPE channel_kind ADD VALUE IF NOT EXISTS 'webpush';

CREATE TABLE IF NOT EXISTS webpush_subscriptions (
    id              UUID PRIMARY KEY,
    notification_id UUID NOT NULL REFERENCES notifications(id) ON DELETE CASCADE,
    -- The push service endpoint URL (unique per browser subscription).
    endpoint        TEXT NOT NULL,
    -- Client public key (p256dh) and auth secret, base64url as sent by the
    -- browser's PushSubscription.
    p256dh          TEXT NOT NULL,
    auth            TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (endpoint)
);

CREATE INDEX IF NOT EXISTS webpush_subscriptions_notification_idx
    ON webpush_subscriptions (notification_id);
