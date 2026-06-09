-- ─── User roles (RBAC) ──────────────────────────────────────────────────────
-- Three-tier role model replacing the boolean is_admin flag:
--   admin    → everything (user mgmt, settings, security, api-keys, proxies,
--              audit-log + all monitor/incident/maintenance/status-page CRUD)
--   editor   → all monitor / incident / maintenance / status-page / notification
--              / tag / folder CRUD + test actions, but NOT the admin-only
--              surfaces above.
--   readonly → GET/read only everywhere; no create/update/delete/test.
--
-- `is_admin` is intentionally KEPT (not dropped) for one release as a
-- rollback shim. `role` is now the authoritative column; the application
-- writes both in sync (is_admin = (role = 'admin')). Drop is_admin in a
-- follow-up migration once the rollback window has closed.

CREATE TYPE user_role AS ENUM ('admin', 'editor', 'readonly');

ALTER TABLE users
  ADD COLUMN role user_role NOT NULL DEFAULT 'editor';

-- Existing admins keep admin; everyone else defaults to editor (the column
-- default) which preserves their prior write capability.
UPDATE users SET role = 'admin' WHERE is_admin = true;

COMMENT ON COLUMN users.role IS
  'Authoritative RBAC role. is_admin is a derived rollback shim kept one release.';
COMMENT ON COLUMN users.is_admin IS
  'DEPRECATED derived shim — kept one release for rollback. role is authoritative.';
