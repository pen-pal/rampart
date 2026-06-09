-- ─── API-key scopes (per-key authorization) ─────────────────────────────────
-- Until now every API key authenticated a request but carried FULL access —
-- a real security gap. This migration adds a single, authoritative `scope`
-- per key, mirroring the RBAC role semantics (migration 0048) but at the key
-- level:
--   read   → GET/HEAD only        (maps to Role::Readonly)
--   write  → read + mutations     (maps to Role::Editor — like an editor)
--   admin  → everything, incl. the admin-only routes (maps to Role::Admin)
--
-- The original schema (0001) shipped a `scopes TEXT[]` column that the create
-- route accepted (`scopes: []`) but NOTHING ever enforced — it was advisory
-- only (see the old doc-comment on rampart_core::api_key). We reconcile that
-- free-form array down to ONE constrained `scope` enum-string for simplicity:
-- it's a closed set with clear precedence, which the array never expressed.
--
-- Backfill: every PRE-EXISTING key had full, unscoped access. Silently
-- downgrading a live automation key to 'read' would break callers in prod, so
-- we backfill existing rows to 'admin' (preserve prior behaviour). New keys
-- default to 'read' (least privilege) — see the column DEFAULT below.
--
-- `scopes` (the array) is intentionally KEPT for one release as a rollback
-- shim, exactly as 0048 kept `is_admin`. `scope` is now authoritative; the
-- application reads/writes only `scope`. Drop `scopes` in a follow-up.

ALTER TABLE api_keys
  ADD COLUMN scope TEXT NOT NULL DEFAULT 'read'
    CHECK (scope IN ('read', 'write', 'admin'));

-- Existing keys had full access before scopes were enforced — don't downgrade
-- live keys out from under their callers. New keys (post-migration) get the
-- 'read' column default instead.
UPDATE api_keys SET scope = 'admin';

COMMENT ON COLUMN api_keys.scope IS
  'Authoritative per-key scope: read|write|admin. Mirrors RBAC roles at the key level. Backfilled to admin for pre-enforcement keys.';
COMMENT ON COLUMN api_keys.scopes IS
  'DEPRECATED free-form advisory array — never enforced. Kept one release for rollback; scope is authoritative.';
