-- Multi-tenancy / security hardening: store ingest credentials HASHED at rest.
--
-- Today `ingest_keys.token` (per-org OTLP/prom/RUM/profiles key) and
-- `ingest_tokens.token` (per-status-page webhook token) are stored VERBATIM and
-- resolved by `WHERE token = $1`. This brings them in line with the already-hashed
-- peers `api_keys.key_hash` / `agents.token_hash` (see 0113): we add a
-- `token_hash TEXT` column holding lowercase-hex SHA-256 of the token and backfill
-- it from the existing plaintext. The plaintext `token` column is KEPT this
-- release (Phase A) so a rollback to the previous app build — which reads
-- `WHERE token = $1` — still resolves every credential. A LATER migration drops
-- the plaintext column (Phase D) once every node runs the hash-reading build;
-- that is the point at-rest exposure is actually eliminated.
--
-- NON-BREAKING: the credential value the client presents is UNCHANGED. We only
-- change how the server stores/looks it up. Every already-minted key/token keeps
-- working because we backfill token_hash = SHA-256(plaintext) for every existing
-- row here, then the same release flips the app to hash-compare (with a plaintext
-- fallback so a row created by an old node mid-rolling-deploy still resolves).
--
-- The hex is lowercase to match the Rust side exactly: hex::encode(Sha256)
-- (api_keys.rs sha256_hex) == encode(digest(t,'sha256'),'hex') in pgcrypto.
-- pgcrypto is already enabled (0001_initial.sql). The UNIQUE on token_hash stays
-- GLOBAL (not per-org), mirroring the plaintext UNIQUE it shadows.

-- ── ingest_keys ──────────────────────────────────────────────────────────────
ALTER TABLE ingest_keys ADD COLUMN token_hash TEXT;
UPDATE ingest_keys
   SET token_hash = encode(digest(token, 'sha256'), 'hex')
 WHERE token_hash IS NULL;
CREATE UNIQUE INDEX ingest_keys_token_hash_uidx ON ingest_keys (token_hash);

-- ── ingest_tokens ────────────────────────────────────────────────────────────
ALTER TABLE ingest_tokens ADD COLUMN token_hash TEXT;
UPDATE ingest_tokens
   SET token_hash = encode(digest(token, 'sha256'), 'hex')
 WHERE token_hash IS NULL;
CREATE UNIQUE INDEX ingest_tokens_token_hash_uidx ON ingest_tokens (token_hash);

-- DEFERRED to a follow-up migration (Phase D), NOT here — see
-- docs/design or the 0.152.x CHANGELOG:
--   * ALTER TABLE ingest_keys/ingest_tokens ALTER COLUMN token_hash SET NOT NULL;
--   * DROP COLUMN token (+ its old UNIQUE);
--   * drop the plaintext fallback in find_by_token + the dual-write of `token`;
--   * for ingest_tokens this also removes admin re-show (becomes show-once).
-- Kept separate so this release stays fully reversible (rollback-safe).
