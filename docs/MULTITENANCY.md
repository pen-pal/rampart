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
| 3 | **Read filtering (management API)** | `WHERE org_id = $ctx` on every management query + ID-set endpoint validation; scheduler/notifier given explicit unscoped/same-org helpers. Rolling out per-domain: **3a monitors ✅ (v0.109.0)**, **3b alert-rules + silences ✅ (v0.110.0)**, **3c SLOs + on-call ✅ (v0.111.0)**, **3d notification channels ✅ (v0.112.0)**, **3e detection rules + delivery-log ✅ (v0.113.0)**, **3f escalations ✅ (v0.114.0) — alerting domain read-filtering COMPLETE**, **3g status-pages ✅ (v0.115.0)** (root CRUD; public slug/host paths intentionally unscoped; sections deferred); **3h api-keys + proxies ✅ (v0.116.0)**, **3i agents + scheduled-reports ✅ (v0.117.0) — infra-credentials domain COMPLETE** (agent-token + report-scheduler paths stay unscoped), **3j tags + folders + presets + templates ✅ (v0.118.0) — monitors-core domain COMPLETE**, **3k notification + incident templates ✅ (v0.119.0)**. **3l dashboard aggregates ✅ (v0.120.0)** — summary/history/recent-incidents/recent-errors join their owning root's org_id. **3m status-page sections + ingest tokens ✅ (v0.121.0)** — parent-gap closed (org-gate via the owning page). **Phase-3 org-scoped read/management filtering now COMPLETE for every request surface.** Only-remaining read paths are deferred WITH their phase: telemetry tier (P5 per-org ingest auth), settings/audit_log (P6), `assignable_users` (P4 membership). Next high-value step is **Phase 4** (org CRUD + switcher UI + per-INSERT write-stamping + OIDC→org) — HELD for owner approval; **Phase 6** enforcement-flip also HELD. Secure-by-default: scoped fns keep the plain name, system callers use explicit `*_all`/`*_unscoped` siblings. | in progress |
| 4 | **Org-bound credentials + OIDC→org + org admin UI** | Pin org on api-keys / agents / ingest-tokens / DSNs; OIDC maps identity→org; `/v1/orgs` CRUD + switcher + members UI. | planned |
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
