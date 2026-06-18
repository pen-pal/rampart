-- ─── Multi-tenancy Phase 6: per-org uniqueness for tenant-scoped name columns ──
--
-- Two tenant_root tables enforce a GLOBAL unique name (from 0001): `tags.name`
-- and `notification_templates.name`. In a multi-tenant world a name is unique
-- WITHIN an org, not across the whole install — org A and org B should each be
-- able to have a "prod" tag or a "PagerDuty critical" template. This migration
-- swaps the two global UNIQUE(name) constraints for composite UNIQUE(org_id, name)
-- indexes.
--
-- Behaviour-identical for a Default-only install: every row is in the Default org,
-- so UNIQUE(org_id, name) degenerates to exactly the same invariant the global
-- UNIQUE(name) enforced — no name that was legal before is illegal now, and vice
-- versa. A pre-flight audit (run before this migration) confirmed zero
-- per-(org_id, name) duplicates, so the new indexes build without conflict.
--
-- DELIBERATELY LEFT GLOBAL (NOT swapped) — these are URL / DSN / secret / domain
-- keys that must stay unique across the whole install regardless of org:
--   organizations.slug, status_pages.slug, status_pages.custom_domain,
--   error_projects.slug, error_projects.public_key (the public DSN shipped in
--   browser JS), monitors.push_token, api_keys.key_hash, agents.token_hash,
--   ingest_keys.token, ingest_tokens.token, users.email.
-- All other tenant_root name columns (monitor_groups.name, scheduled_reports.name,
-- the rule/SLO/schedule names, …) are already plain NOT NULL (not UNIQUE), so
-- they need no change.
--
-- Reversibility caveat: this is cleanly reversible ONLY while the install is still
-- single-org. To revert: DROP the composite index and re-add UNIQUE(name). Once a
-- second org has created a row reusing a name that already exists in another org,
-- re-adding the global UNIQUE(name) would fail on the cross-org duplicate — so the
-- revert is effectively one-way after real multi-org data lands. Ship this before
-- onboarding a second org if a clean rollback path must be preserved.

-- tags.name → (org_id, name)
ALTER TABLE tags DROP CONSTRAINT tags_name_key;
CREATE UNIQUE INDEX tags_org_name_uidx ON tags (org_id, name);

-- notification_templates.name → (org_id, name)
ALTER TABLE notification_templates DROP CONSTRAINT notification_templates_name_key;
CREATE UNIQUE INDEX notification_templates_org_name_uidx ON notification_templates (org_id, name);
