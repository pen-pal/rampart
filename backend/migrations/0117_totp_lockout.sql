-- ─── TOTP / recovery-code brute-force lockout ───────────────────────────────
--
-- The 2FA verify step (`/v1/auth/2fa/verify`) re-issued a fresh challenge token
-- on every wrong code with no failure counter, so a caller who cleared the
-- password gate could loop tokens and brute-force a 6-digit TOTP (10^6 space) or
-- the recovery codes — an MFA-bypass / account-takeover path. These columns back
-- a durable per-account attempt cap: after N consecutive failures the account is
-- locked out of the 2FA step for a cooldown; a success clears both.
--
-- Durable (in the row, not in-process) so a restart can't reset an attacker's
-- counter, and so the lockout survives across replicas.
ALTER TABLE users
    ADD COLUMN totp_failed_attempts INT NOT NULL DEFAULT 0,
    ADD COLUMN totp_locked_until    TIMESTAMPTZ;
