-- ─── Row-Level Security, slice 5: the helper + policies (DORMANT) ────────────
--
-- Defines the per-tenant isolation policies but does NOT enable or force RLS on
-- any table. Postgres IGNORES policies until a table has ROW LEVEL SECURITY
-- enabled, so these policies are completely inert: zero enforcement, zero
-- behaviour change. They exist so the SQL can be reviewed and tested (notably
-- the empty-GUC → no-500 property) with no risk. Turning them on
-- (ALTER TABLE … ENABLE/FORCE ROW LEVEL SECURITY) is a separate, deliberate
-- operator step (slice 7), intentionally NOT shipped here.
--
-- The policies read the per-request org from a GUC set by the app's
-- before_acquire hook (`set_config('app.current_org', <org>, false)`), via the
-- helper below. System loops never set the GUC and run as the BYPASSRLS owner.

-- Resolve the current request's org from the `app.current_org` GUC.
--   * `current_setting(..., true)` -> missing GUC returns NULL (not an error).
--   * NULLIF(..., '') -> the hook resets the GUC to '' between requests; map
--     that empty string to NULL so the cast never sees ''.
--   * NULL::uuid means "no org bound": an `org_id = NULL` predicate is NULL
--     (not true), so a SELECT under an empty GUC returns ZERO rows instead of
--     raising 22P02 (invalid uuid) — i.e. no 500. STABLE so the planner can
--     cache it within a statement.
CREATE OR REPLACE FUNCTION app_current_org() RETURNS uuid
    LANGUAGE sql STABLE
    AS $$ SELECT NULLIF(current_setting('app.current_org', true), '')::uuid $$;

-- ── Tenant-root tables (own org_id column, from 0108) ───────────────────────
-- One uniform policy per table: a row is visible/writable iff its org_id equals
-- the bound org. WITH CHECK mirrors USING so an INSERT/UPDATE can't stamp a
-- foreign org. (heartbeats + error_events are deliberately EXCLUDED: no org_id
-- and the highest-volume tables; they inherit isolation via their parent's
-- scope at the app layer.)
DO $$
DECLARE
    t text;
    roots text[] := ARRAY[
        -- monitoring core
        'monitors', 'monitor_groups', 'monitor_presets', 'monitor_templates', 'tags',
        -- notification / alerting
        'notifications', 'notification_templates', 'escalation_policies',
        'on_call_schedules', 'silences', 'delivery_log',
        -- alert rules / SLOs
        'metric_rules', 'telemetry_alert_rules', 'detection_rules', 'slos',
        -- status pages / incidents
        'status_pages', 'incident_templates',
        -- maintenance
        'maintenance', 'maintenance_windows',
        -- telemetry tiers
        'logs', 'spans', 'metric_samples', 'rum_events', 'profiles', 'error_projects',
        -- misc independently-owned resources
        'deploy_markers', 'scheduled_reports', 'api_keys', 'agents', 'proxies'
    ];
BEGIN
    FOREACH t IN ARRAY roots LOOP
        EXECUTE format('DROP POLICY IF EXISTS org_isolation ON %I', t);
        EXECUTE format(
            'CREATE POLICY org_isolation ON %I '
            || 'USING (org_id = app_current_org()) '
            || 'WITH CHECK (org_id = app_current_org())',
            t);
    END LOOP;
END
$$;

-- ── Low-volume CHILD tables (no own org_id; gated via parent's org_id) ───────
-- A subquery resolves the parent row's org and compares to the bound org.
-- USING/WITH CHECK both reference the parent so reads and writes are gated.

-- incidents -> status_pages.org_id (via status_page_id)
DROP POLICY IF EXISTS org_isolation ON incidents;
CREATE POLICY org_isolation ON incidents
    USING (status_page_id IN (SELECT id FROM status_pages WHERE org_id = app_current_org()))
    WITH CHECK (status_page_id IN (SELECT id FROM status_pages WHERE org_id = app_current_org()));

-- status_page_sections -> status_pages.org_id (via status_page_id)
DROP POLICY IF EXISTS org_isolation ON status_page_sections;
CREATE POLICY org_isolation ON status_page_sections
    USING (status_page_id IN (SELECT id FROM status_pages WHERE org_id = app_current_org()))
    WITH CHECK (status_page_id IN (SELECT id FROM status_pages WHERE org_id = app_current_org()));

-- escalation_episodes -> escalation_policies.org_id (via policy_id)
DROP POLICY IF EXISTS org_isolation ON escalation_episodes;
CREATE POLICY org_isolation ON escalation_episodes
    USING (policy_id IN (SELECT id FROM escalation_policies WHERE org_id = app_current_org()))
    WITH CHECK (policy_id IN (SELECT id FROM escalation_policies WHERE org_id = app_current_org()));

-- detection_findings -> detection_rules.org_id (via rule_id)
DROP POLICY IF EXISTS org_isolation ON detection_findings;
CREATE POLICY org_isolation ON detection_findings
    USING (rule_id IN (SELECT id FROM detection_rules WHERE org_id = app_current_org()))
    WITH CHECK (rule_id IN (SELECT id FROM detection_rules WHERE org_id = app_current_org()));

-- NOTE: NO `ALTER TABLE … ENABLE/FORCE ROW LEVEL SECURITY` anywhere in this
-- migration. The policies above are defined but DORMANT. Slice 7 (not built)
-- flips them on.
