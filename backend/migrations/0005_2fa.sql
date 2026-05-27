-- TOTP-based two-factor auth.
--
-- users.totp_secret already exists from migration 0001. We add an
-- explicit enabled flag so a half-finished enrollment (secret stored
-- but never confirmed with a valid code) doesn't lock the user out.
-- Recovery codes live in their own table, hashed exactly like API keys
-- — they're a one-shot escape hatch when the authenticator is lost.

ALTER TABLE users
    ADD COLUMN IF NOT EXISTS totp_enabled BOOLEAN NOT NULL DEFAULT FALSE;

CREATE TABLE IF NOT EXISTS totp_recovery_codes (
    id        UUID PRIMARY KEY,
    user_id   UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    code_hash TEXT NOT NULL,
    used_at   TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS trc_user_unused_idx
    ON totp_recovery_codes (user_id)
    WHERE used_at IS NULL;

-- Hashes are SHA-256 hex, so they're unique per code. Globally-unique
-- index lets us reject duplicates without scanning per-user.
CREATE UNIQUE INDEX IF NOT EXISTS trc_code_hash_idx
    ON totp_recovery_codes (code_hash);
