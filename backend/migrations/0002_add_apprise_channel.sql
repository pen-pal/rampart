-- Add the 'apprise' value to channel_kind.
--
-- The Apprise gateway is a single channel kind that proxies to ~80
-- upstream services through the user's apprise-api sidecar
-- (https://github.com/caronc/apprise-api). One Rampart "channel"
-- holds a list of apprise:// URLs and forwards to all of them on flip.
--
-- ADD VALUE is additive and safe; downgrade is not supported by Postgres
-- (you can't remove an enum value once data references it).

ALTER TYPE channel_kind ADD VALUE IF NOT EXISTS 'apprise';
