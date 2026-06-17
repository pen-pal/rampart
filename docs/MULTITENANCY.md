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
| 2 | **Stamp `org_id` at write time** | Nullable `org_id` (default Default-org) on every tenant root; children inherit via FK; every INSERT stamps it. Reads still global. | planned |
| 3 | **Read filtering (management API)** | `WHERE org_id = $ctx` on every management query + ID-set endpoint validation; scheduler/notifier given explicit unscoped/same-org helpers. | planned |
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
