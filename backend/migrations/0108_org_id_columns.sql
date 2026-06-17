-- ─── Multi-tenancy Phase 2: per-resource org_id columns (behaviour-identical) ─
--
-- Phase 1 (0107) introduced the tenant ROOT (`organizations`), the user↔org
-- membership join, and a single well-known "Default" org. This migration adds
-- the ownership column `org_id` to every tenant_root resource table — the
-- tables a tenant directly owns and that Phase 3's read filter must scope.
--
-- This is purely additive and behaviour-identical:
--   * The column is NULLABLE (the NOT NULL tightening is deferred to Phase 6).
--   * It has a CONSTANT DEFAULT of the well-known Default org UUID
--     ('00000000-0000-0000-0000-000000000001' = Uuid::from_u128(1)), so every
--     existing row is owned by the Default org and every new row defaults to it
--     with NO table rewrite and NO backfill UPDATE.
--   * The FK is a plain REFERENCES (ON DELETE RESTRICT, the default), so an org
--     cannot be dropped while it still owns rows.
--   * NO index is created here — the org_id indexes land alongside the read
--     filters in Phase 3, so this stays a fast metadata-only change.
--
-- Scope decisions:
--   * CHILD tables (heartbeats, error_events, status_page_*, incidents, …) get
--     NO column — they inherit org transitively via their NOT-NULL FK to a root.
--   * JOIN tables (monitor_tags, *_notifications, *_monitors, …) get NO column —
--     both sides are already scoped roots.
--   * GLOBAL tables (users, sessions, organizations, org_members, totp_recovery_
--     codes, webpush_subscriptions, api_key_rate_usage, digest_buffer) get NO
--     column — they are tenancy machinery, per-user, or transient/derived infra.
--   * settings and audit_log are GLOBAL-SPECIAL: they gain an org_id as a
--     filter/partition column in a dedicated later phase (PK reshaping / hash
--     chain / derived-from-resource), NOT via this generic nullable ALTER.
--
-- Leak note: delivery_log is a tenant_root here (not a child) on purpose — its
-- only FK (notification_id) is ON DELETE SET NULL and monitor_id has no FK, so
-- rows orphan and cannot be safely scoped via a parent; it is durable and
-- read-exposed (GET /v1/delivery-log), so it must carry its own org_id.

-- ── Monitoring core ─────────────────────────────────────────────────────────
ALTER TABLE monitors             ADD COLUMN org_id uuid DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES organizations(id);
ALTER TABLE monitor_groups       ADD COLUMN org_id uuid DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES organizations(id);
ALTER TABLE monitor_presets      ADD COLUMN org_id uuid DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES organizations(id);
ALTER TABLE monitor_templates    ADD COLUMN org_id uuid DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES organizations(id);
ALTER TABLE tags                 ADD COLUMN org_id uuid DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES organizations(id);

-- ── Notification / alerting ───────────────────────────────────────────────────
ALTER TABLE notifications        ADD COLUMN org_id uuid DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES organizations(id);
ALTER TABLE notification_templates ADD COLUMN org_id uuid DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES organizations(id);
ALTER TABLE escalation_policies  ADD COLUMN org_id uuid DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES organizations(id);
ALTER TABLE on_call_schedules    ADD COLUMN org_id uuid DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES organizations(id);
ALTER TABLE silences             ADD COLUMN org_id uuid DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES organizations(id);
ALTER TABLE delivery_log         ADD COLUMN org_id uuid DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES organizations(id);

-- ── Alert rules / SLOs ────────────────────────────────────────────────────────
ALTER TABLE metric_rules         ADD COLUMN org_id uuid DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES organizations(id);
ALTER TABLE telemetry_alert_rules ADD COLUMN org_id uuid DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES organizations(id);
ALTER TABLE detection_rules      ADD COLUMN org_id uuid DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES organizations(id);
ALTER TABLE slos                 ADD COLUMN org_id uuid DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES organizations(id);

-- ── Status pages / incidents ──────────────────────────────────────────────────
ALTER TABLE status_pages         ADD COLUMN org_id uuid DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES organizations(id);
ALTER TABLE incident_templates   ADD COLUMN org_id uuid DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES organizations(id);

-- ── Maintenance ───────────────────────────────────────────────────────────────
ALTER TABLE maintenance          ADD COLUMN org_id uuid DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES organizations(id);
ALTER TABLE maintenance_windows  ADD COLUMN org_id uuid DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES organizations(id);

-- ── Telemetry tiers (ingested streams) ────────────────────────────────────────
ALTER TABLE logs                 ADD COLUMN org_id uuid DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES organizations(id);
ALTER TABLE spans                ADD COLUMN org_id uuid DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES organizations(id);
ALTER TABLE metric_samples       ADD COLUMN org_id uuid DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES organizations(id);
ALTER TABLE rum_events           ADD COLUMN org_id uuid DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES organizations(id);
ALTER TABLE profiles             ADD COLUMN org_id uuid DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES organizations(id);
ALTER TABLE error_projects       ADD COLUMN org_id uuid DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES organizations(id);

-- ── Misc independently-owned resources ────────────────────────────────────────
ALTER TABLE deploy_markers       ADD COLUMN org_id uuid DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES organizations(id);
ALTER TABLE scheduled_reports    ADD COLUMN org_id uuid DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES organizations(id);
ALTER TABLE api_keys             ADD COLUMN org_id uuid DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES organizations(id);
ALTER TABLE agents               ADD COLUMN org_id uuid DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES organizations(id);
ALTER TABLE proxies              ADD COLUMN org_id uuid DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES organizations(id);
