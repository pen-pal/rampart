-- ─── Per-user UI preferences ─────────────────────────────────────────────────
-- A single JSONB blob hung off the users row holds opaque, client-shaped UI
-- state: saved dashboard filter views + the dashboard's default folder. We
-- keep this deliberately schema-less so the frontend can evolve the shape
-- (add view fields, new pref keys) without a migration each time. The backend
-- only ever round-trips the blob — it does not interpret or validate it.
--
-- Expected shape (informational only; NOT enforced):
--   {
--     "saved_views": [
--       { "id": "...", "name": "Prod down", "tags": ["uuid", ...],
--         "folder": "uuid|null", "search": "..." }
--     ],
--     "default_folder_id": "uuid|null"
--   }

ALTER TABLE users
  ADD COLUMN prefs JSONB NOT NULL DEFAULT '{}'::jsonb;

COMMENT ON COLUMN users.prefs IS
  'Opaque per-user UI preferences (saved dashboard views + default folder). Client-shaped; the backend round-trips it without interpreting.';
