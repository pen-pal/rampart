-- Tamper-evident audit log via a per-row hash chain.
--
-- Each new row stores `hash = HMAC-SHA256(key, prev_hash || canonical(row))`
-- where key is RAMPART_SECRET_KEY (kept OUTSIDE the database) and prev_hash is
-- the previous row's hash. Editing, deleting or reordering any row breaks the
-- chain from that point on, and — because the MAC key isn't in the DB — a party
-- who can write to the database can't recompute valid hashes to cover it up.
-- (Without a key set, a plain SHA-256 chain is used: still detects naive/
-- accidental tampering, just not a key-holding adversary.)
--
-- Pre-existing rows keep NULL hashes; the chain protects entries written after
-- this migration. Columns are nullable so the append path can backfill the
-- hash in the same transaction as the insert (BIGSERIAL id + ts are needed in
-- the hash, so they're known only after the INSERT).
ALTER TABLE audit_log
    ADD COLUMN prev_hash TEXT,
    ADD COLUMN hash      TEXT;
