# Multi-tenancy

Rampart shipped single-tenant by design — one install, one team, no org
concept (see `backend/migrations/0001_initial.sql`). Multi-tenancy
(organizations / workspaces, with per-resource ownership and cross-tenant
isolation) is the #1 enterprise / MSP requirement, and it is being introduced
as a **phased, correctness-first epic**: the system is never left in a
half-scoped state where one tenant could read another's data.

## Membership model

**User ↔ org many-to-many**, with the RBAC role living on the membership row
(`org_members.role`), not on the user. This is what lets a single account be
`admin` in one org and `readonly` in another — the MSP / reseller "one admin
across customers" case. The global `users.role` stays the source of truth for
one release (mirrored onto the Default-org membership) and is then deprecated.

## The Default org

Every install has a well-known **Default org**
(`00000000-0000-0000-0000-000000000001`, i.e. `Uuid::from_u128(1)`). All
pre-multi-tenancy data, the first user, and any request without a resolved org
belong to it. Until enforcement is flipped on (Phase 6), the Default org is the
only live org, so behaviour is identical to the single-tenant build.

## Phases

Each phase is independently correct and shippable; none introduces a
cross-tenant leak on its own.

| # | Phase | Gist | Status |
|---|-------|------|--------|
| 1 | **Foundation** | `organizations` + `org_members` tables, `sessions.active_org_id`, Default-org backfill, `OrgContext` plumbed through auth. Behaviour-identical. | ✅ shipped (v0.107.0) |
| 2 | **Per-resource `org_id` columns** | Nullable `org_id` (DEFAULT Default-org) on all 30 tenant roots; children inherit via FK; no behaviour change. Explicit per-INSERT stamping from `OrgContext` folds into Phase 4 (where non-default orgs first exist — until then the column DEFAULT is provably equivalent). | ✅ shipped (v0.108.0) |
| 3 | **Read filtering (management API)** | `WHERE org_id = $ctx` on every management query + ID-set endpoint validation; scheduler/notifier given explicit unscoped/same-org helpers. Rolling out per-domain: **3a monitors ✅ (v0.109.0)**, **3b alert-rules + silences ✅ (v0.110.0)**, **3c SLOs + on-call ✅ (v0.111.0)**, **3d notification channels ✅ (v0.112.0)**, **3e detection rules + delivery-log ✅ (v0.113.0)**, **3f escalations ✅ (v0.114.0) — alerting domain read-filtering COMPLETE**, **3g status-pages ✅ (v0.115.0)** (root CRUD; public slug/host paths intentionally unscoped; sections deferred); **3h api-keys + proxies ✅ (v0.116.0)**, **3i agents + scheduled-reports ✅ (v0.117.0) — infra-credentials domain COMPLETE** (agent-token + report-scheduler paths stay unscoped), **3j tags + folders + presets + templates ✅ (v0.118.0) — monitors-core domain COMPLETE**, **3k notification + incident templates ✅ (v0.119.0)**. **3l dashboard aggregates ✅ (v0.120.0)** — summary/history/recent-incidents/recent-errors join their owning root's org_id. **3m status-page sections + ingest tokens ✅ (v0.121.0)** — parent-gap closed (org-gate via the owning page). **NOTE: the post-3m "complete for every surface" claim was premature** — a follow-up audit (2026-06-17) found several authenticated management surfaces still operate on a tenant-root resource (or child) by id with no org check. Closing them per-surface as **3n+**: **3n incidents ✅ (v0.122.0)** — `/v1/status-pages/{page}/incidents` + top-level `/v1/incidents/{id}` (update/delete/resolve/updates) now gate through the owning page (public Atom feed + webhook ingest + seed stay unscoped); test `incidents_isolated_via_owning_page`. **3o error-tracking ✅ (v0.123.0)** — `error_projects` `list`/`update`/`delete` filter `WHERE org_id`; `project_in_org` + `issue_in_org` 404-gates front every project- and issue-keyed handler (issues/histogram/sourcemaps + `/v1/error-issues/{id}` detail/stats/users/events/resolve/ignore/unresolve/assign); DSN/RUM ingest + prune + trace-correlation stay unscoped, `assignable_users` → P4; test `error_projects_isolated_across_orgs`. **3p bulk monitor ops ✅ (v0.124.0)** — `bulk_edit`/`bulk_edit_preview` resolve each id `WHERE id=$ AND org_id=$` (cross-org ids land in the existing `skipped` bucket), `set_active_by_tag` flips only `WHERE org_id=$`; test `bulk_edit_skips_cross_org_monitors`. **3q attach/detach junctions ✅ (v0.125.0)** — tag↔monitor, channel↔monitor, monitor↔monitor deps (+ the bulk junction arms) gate BOTH ends via `monitors::get`/`tags::get`/`notifications::get`; route-only, no db-sig change; test `monitor_junctions_isolated`. **3r tag-routing ✅ (v0.126.0)** — all 13 `routes/routing.rs` handlers gate the folder (`monitor_groups::in_org`), channel (`notifications::get`), monitor (`monitors::get`) and tag (`tags::get`); notifier `resolve_channels_for_monitor` stays unscoped; test `tag_routing_isolated`. **3s escalation episodes ✅ (v0.127.0)** — episode list + ack-by-id join `escalation_episodes→escalation_policies` and filter `p.org_id` (`list_open_for_org`, `episode_in_org`); monitor-keyed `episode`/`ack` gate via `monitors::get`; scheduler/test fns stay unscoped; test `escalation_episodes_isolated`. **3t detection findings ✅ (v0.128.0)** — findings feed + ack-by-id join `detection_findings→detection_rules` and filter `r.org_id` (`list_findings_for_org`, `finding_in_org`); unscoped `list_findings`/`fetch_since` (SIEM export) stay; test `detection_findings_isolated`. **3u flagged monitor-keyed reads ✅ (v0.129.0)** — `reliability`/`heartbeats`/`heartbeats_csv` + on_call `current` now gate via `monitors::get`/`on_call::get`; test `monitor_heartbeat_reads_isolated`. Also fixed a pre-existing red `delivery_log` CSV-export unit test (missing OrgContext extension since 3e). **ENTIRE PHASE-3 AUDIT SWEEP (3n–3u) COMPLETE** — 11-test cross-org isolation suite + full api+db suite green. Every authenticated management/read surface across all tenant-root domains is now org-gated. **Remaining MT work is HELD/deferred-with-phase only:** P4 (org CRUD+switcher UI, per-INSERT write-stamping, OIDC→org, assignable_users), P5 (ingest tier + telemetry reads + search/aggregate), P6 (enforcement flip: NOT NULL / drop Default fallback / per-org uniqueness / RLS / settings + audit_log). Flagged/tier-ambiguous: heartbeats `reliability`, on_call `current`. Only paths deferred WITH their phase: telemetry tier (P5 per-org ingest auth), settings/audit_log (P6), `assignable_users` (P4 membership). After the 3n+ sweep, next high-value step is **Phase 4** (org CRUD + switcher UI + per-INSERT write-stamping + OIDC→org) — HELD for owner approval; **Phase 6** enforcement-flip also HELD. Secure-by-default: scoped fns keep the plain name, system callers use explicit `*_all`/`*_unscoped` siblings. | in progress |
| 4 | **Org CRUD + switcher UI + write-stamping + OIDC→org + per-org RBAC** | Owner green-lit P4→P5→P6 (2026-06-18). Sliced 4a-4g: **4a per-INSERT org-stamping ✅ (v0.130.0)** — every tenant-root create stamps `org_id` explicitly (param threaded from `OrgContext`; token-less ingest stamps Default w/ `// P5` marker; `/v1/metrics/ingest` stamps caller org; `delivery_log` COALESCE-from-notification; UNNEST via ARRAY_FILL); behaviour-identical single-org; test `org_write_stamping.rs`. **4b db primitives + users.role↔Default-membership mirror ✅ (v0.131.0)** — orgs `update`/`get_by_slug`/`remove_member`/`count_admins`/`create_with_owner`; `users::set_role`/`set_admin` now mirror the role onto the Default-org membership in-tx (was stale → would break 4e single-org RBAC); behaviour-safe; **4c `/v1/orgs` CRUD + members ✅ (v0.133.0)** — routes/orgs.rs in self_service, per-org authz via `require_org_role` (non-member 404 / under-priv 403, `Role::at_least`); any user creates (creator=Admin, atomic); rename/members Admin-only; last-admin protection (409); add-member by email (existing users only); no delete; `Role::rank`, `users::by_email`, `orgs::list_members_detailed` added; 5 tests; **4d switcher + me() enrichment ✅ (v0.134.0)** — `POST /v1/orgs/{id}/switch` sets `sessions.active_org_id` (membership-gated, 404 non-member, 401 no-cookie); `/v1/auth/me` returns org list + active_org_id (Default fallback); becomes effective in 4e; test `switch_active_org_and_me_reflects_it`; **4e per-org RBAC ✅ (v0.135.0)** — require_session resolves the active-org role from org_members onto user.role (existing guards enforce per-org, no guard edits); makes the 4d switch effective (Readonly-in-org → writes 403); non-member/unset active_org → Default fallback; user.is_admin stays global; bearer path unchanged; test `per_org_role_gates_writes`; **4f OIDC→org ✅ (v0.136.0)** — optional `RAMPART_OIDC_ORG_CLAIM` (groups/custom-org/hd) → org by slug at login (slug-normalised), grants membership at DEFAULT_ROLE, first match = active org; idempotent re-sync; unmatched ignored (no auto-create/deny → Default); unset = pre-4f behaviour; pure unit-tested resolver; **4g** frontend switcher + Organizations admin page. No enforcement flip (that's P6). | in progress |
| 5 | **Tenant the ingest tier** (riskiest) | Per-org ingest credentials resolve org from the credential (never the body); stamp `org_id` on spans/logs/profiles/RUM/metrics/errors; org-filter every search + aggregate. | planned |
| 6 | **Flip enforcement** | `SET NOT NULL`, per-org uniqueness, drop the Default-org fallback, add Postgres RLS as defense-in-depth. | planned |

## Leak traps (tracked, handled in the phase noted)

- **Ingest tier** (biggest pre-existing gap): OTLP traces/logs, profiles, RUM,
  Prometheus remote-write, Sentry errors are written with no owner today →
  Phase 5 resolves org from the credential, not the attacker-controllable
  `service.name`.
- **`/metrics` scrape**: deliberately kept instance-global (org-scoping it would
  leak per-tenant volume as a side channel).
- **Public status pages**: resolved by slug / Host header outside the session
  layer — never gain a "list all pages" endpoint.
- **Search surfaces**: logs full-text + traces service-map must org-filter (P5).
- **ID-set endpoints** (bulk/bulk-edit/attach/detach) must validate every
  client-supplied UUID belongs to the active org (P3).
- **Scheduler** runs with no request context → explicit unscoped
  `list_all_for_scheduler` (P3). **Notifier** fan-out validates channels are
  same-org as the triggering monitor/project (P3).
- **Audit hash chain**: `org_id` is a filter column only — the single global
  HMAC chain is preserved (forking per org would break previous-hash linkage).
