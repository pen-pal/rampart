-- ─── Multi-tenancy Phase 1: foundation (behaviour-identical) ────────────────
--
-- Rampart has been single-tenant by design (see 0001_initial.sql) — one
-- install, one team, no org concept. This migration introduces the tenant
-- ROOT (`organizations`) and the user↔org membership join, but changes NO
-- behaviour: every existing row, user, and session is backfilled into a single
-- well-known "Default" org, and no query yet filters by org. Read filtering,
-- per-resource org_id columns, and enforcement land in later phases — this is
-- purely the additive, reversible foundation (drop the two tables + the
-- nullable column to roll back).
--
-- Membership model: user↔org MANY-TO-MANY with the role on the membership row
-- (not single-org-per-user). This is what later enables MSP / reseller "one
-- admin across customers"; the role on the membership lets a user be admin in
-- one org and readonly in another. For now membership.role == users.role for
-- everyone (one org), so RBAC stays byte-identical.

CREATE TABLE organizations (
    id          uuid PRIMARY KEY,
    -- URL-safe, lowercased slug; same shape rule as status_pages.slug.
    slug        text        NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9-]{2,40}$'),
    name        text        NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE org_members (
    org_id      uuid        NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id     uuid        NOT NULL REFERENCES users(id)         ON DELETE CASCADE,
    -- Per-org role (reuses the user_role enum from 0048). This is the
    -- authoritative role *within this org*; the global users.role stays the
    -- source of truth this release and is mirrored here at write time.
    role        user_role   NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (org_id, user_id)
);
-- "which orgs is this user in" — the membership / switcher lookup.
CREATE INDEX org_members_user_idx ON org_members (user_id);

-- Which org a browser session is currently acting in. NULL means "not yet
-- chosen" and the auth layer falls back to the Default org; a later phase
-- adds the org-switcher that writes this column.
ALTER TABLE sessions
  ADD COLUMN active_org_id uuid REFERENCES organizations(id) ON DELETE SET NULL;

-- The Default org: a fixed, well-known UUID (= Uuid::from_u128(1)) so the
-- application can reference it as a constant without a lookup.
INSERT INTO organizations (id, slug, name)
VALUES ('00000000-0000-0000-0000-000000000001', 'default', 'Default')
ON CONFLICT (id) DO NOTHING;

-- Backfill: every existing user joins the Default org with their current
-- (global) role; every existing session points at the Default org.
INSERT INTO org_members (org_id, user_id, role)
SELECT '00000000-0000-0000-0000-000000000001', id, role
FROM users
ON CONFLICT (org_id, user_id) DO NOTHING;

UPDATE sessions
SET active_org_id = '00000000-0000-0000-0000-000000000001'
WHERE active_org_id IS NULL;
