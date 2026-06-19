-- ─── Multi-tenancy Phase 6: enforce org_id NOT NULL + drop the Default DEFAULT ─
--
-- Phases 2-5 added a NULLABLE org_id (0108) with a CONSTANT DEFAULT of the
-- well-known Default org, stamped it at every write (Phase 4 / org_write_
-- stamping.rs), and scoped every read/aggregate to it (Phases 3 & 5). The column
-- DEFAULT was only ever a safety net for un-stamped INSERTs — and there are none
-- left: every writer in rampart-db binds org_id explicitly, proven by the
-- org_write_stamping regression suite.
--
-- This migration flips the enforcement:
--   * SET NOT NULL  — a forgotten stamp now FAILS LOUD (a constraint error)
--                     instead of silently re-filing the row into the Default org.
--   * DROP DEFAULT  — removes the silent-Default footgun entirely. Both go in the
--                     SAME migration so no window exists where a NULL could land.
--
-- Behaviour-identical for a Default-only install: every existing row already has
-- org_id = Default (the 0108 column DEFAULT applied to all rows at ALTER time),
-- and every new row is stamped explicitly, so NOT NULL is already satisfied and
-- dropping the DEFAULT changes nothing observable.
--
-- Reversible: `ALTER TABLE <t> ALTER COLUMN org_id DROP NOT NULL;` and
-- `... SET DEFAULT '00000000-0000-0000-0000-000000000001';` restore the prior
-- state with no data loss.
--
-- Lock note: SET NOT NULL validates with a table scan under ACCESS EXCLUSIVE.
-- Per the 0111 precedent ("Rampart applies them at startup where a brief lock is
-- fine") and the single-instance deployment model, this is acceptable at boot.
-- Operators running very large telemetry tiers on a busy cluster should apply
-- this during a maintenance window.
--
-- Indexes are deliberately NOT added here: the 5 high-volume telemetry tiers
-- already carry composite (org_id, <time>) indexes from 0111, and a single-value
-- org_id index on the low-volume management tables (every row is Default today)
-- has zero selectivity until real multi-org data exists — it would be pure write/
-- disk overhead. Per-org management-table indexes belong in a future migration,
-- alongside the data that makes them selective.
--
-- The `org_id` set is exactly the 30 tenant_root tables from 0108 (NOT the child/
-- join/global tables, which inherit org transitively or are tenancy machinery).
-- `delivery_log` is included: its org_id is computed via a COALESCE floor to
-- Default (delivery_log.rs) which can never yield NULL, so NOT NULL is safe — but
-- that COALESCE is load-bearing and must NOT be removed.

-- ── Monitoring core ─────────────────────────────────────────────────────────
ALTER TABLE monitors             ALTER COLUMN org_id SET NOT NULL, ALTER COLUMN org_id DROP DEFAULT;
ALTER TABLE monitor_groups       ALTER COLUMN org_id SET NOT NULL, ALTER COLUMN org_id DROP DEFAULT;
ALTER TABLE monitor_presets      ALTER COLUMN org_id SET NOT NULL, ALTER COLUMN org_id DROP DEFAULT;
ALTER TABLE monitor_templates    ALTER COLUMN org_id SET NOT NULL, ALTER COLUMN org_id DROP DEFAULT;
ALTER TABLE tags                 ALTER COLUMN org_id SET NOT NULL, ALTER COLUMN org_id DROP DEFAULT;

-- ── Notification / alerting ───────────────────────────────────────────────────
ALTER TABLE notifications        ALTER COLUMN org_id SET NOT NULL, ALTER COLUMN org_id DROP DEFAULT;
ALTER TABLE notification_templates ALTER COLUMN org_id SET NOT NULL, ALTER COLUMN org_id DROP DEFAULT;
ALTER TABLE escalation_policies  ALTER COLUMN org_id SET NOT NULL, ALTER COLUMN org_id DROP DEFAULT;
ALTER TABLE on_call_schedules    ALTER COLUMN org_id SET NOT NULL, ALTER COLUMN org_id DROP DEFAULT;
ALTER TABLE silences             ALTER COLUMN org_id SET NOT NULL, ALTER COLUMN org_id DROP DEFAULT;
ALTER TABLE delivery_log         ALTER COLUMN org_id SET NOT NULL, ALTER COLUMN org_id DROP DEFAULT;

-- ── Alert rules / SLOs ────────────────────────────────────────────────────────
ALTER TABLE metric_rules         ALTER COLUMN org_id SET NOT NULL, ALTER COLUMN org_id DROP DEFAULT;
ALTER TABLE telemetry_alert_rules ALTER COLUMN org_id SET NOT NULL, ALTER COLUMN org_id DROP DEFAULT;
ALTER TABLE detection_rules      ALTER COLUMN org_id SET NOT NULL, ALTER COLUMN org_id DROP DEFAULT;
ALTER TABLE slos                 ALTER COLUMN org_id SET NOT NULL, ALTER COLUMN org_id DROP DEFAULT;

-- ── Status pages / incidents ──────────────────────────────────────────────────
ALTER TABLE status_pages         ALTER COLUMN org_id SET NOT NULL, ALTER COLUMN org_id DROP DEFAULT;
ALTER TABLE incident_templates   ALTER COLUMN org_id SET NOT NULL, ALTER COLUMN org_id DROP DEFAULT;

-- ── Maintenance ───────────────────────────────────────────────────────────────
ALTER TABLE maintenance          ALTER COLUMN org_id SET NOT NULL, ALTER COLUMN org_id DROP DEFAULT;
ALTER TABLE maintenance_windows  ALTER COLUMN org_id SET NOT NULL, ALTER COLUMN org_id DROP DEFAULT;

-- ── Telemetry tiers (ingested streams) ────────────────────────────────────────
ALTER TABLE logs                 ALTER COLUMN org_id SET NOT NULL, ALTER COLUMN org_id DROP DEFAULT;
ALTER TABLE spans                ALTER COLUMN org_id SET NOT NULL, ALTER COLUMN org_id DROP DEFAULT;
ALTER TABLE metric_samples       ALTER COLUMN org_id SET NOT NULL, ALTER COLUMN org_id DROP DEFAULT;
ALTER TABLE rum_events           ALTER COLUMN org_id SET NOT NULL, ALTER COLUMN org_id DROP DEFAULT;
ALTER TABLE profiles             ALTER COLUMN org_id SET NOT NULL, ALTER COLUMN org_id DROP DEFAULT;
ALTER TABLE error_projects       ALTER COLUMN org_id SET NOT NULL, ALTER COLUMN org_id DROP DEFAULT;

-- ── Misc independently-owned resources ────────────────────────────────────────
ALTER TABLE deploy_markers       ALTER COLUMN org_id SET NOT NULL, ALTER COLUMN org_id DROP DEFAULT;
ALTER TABLE scheduled_reports    ALTER COLUMN org_id SET NOT NULL, ALTER COLUMN org_id DROP DEFAULT;
ALTER TABLE api_keys             ALTER COLUMN org_id SET NOT NULL, ALTER COLUMN org_id DROP DEFAULT;
ALTER TABLE agents               ALTER COLUMN org_id SET NOT NULL, ALTER COLUMN org_id DROP DEFAULT;
ALTER TABLE proxies              ALTER COLUMN org_id SET NOT NULL, ALTER COLUMN org_id DROP DEFAULT;
