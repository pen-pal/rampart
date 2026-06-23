# Horizontal sharding (org-keyed / tenant-per-shard) — design & phased plan

Status: **DESIGN / NOT STARTED.** Task #104. This is a design pass and phased
plan only; **no implementation has begun** and the first real phase is
owner-gated. The tone, framing, and honesty bar follow `docs/design/MULTI_DB.md`:
state what survives, state what breaks, and put a number on the cost.

The honest framing, up front: **you almost certainly do not need this yet, and a
single well-tuned Postgres scales much further than intuition suggests.** Org
sharding is not a feature you turn on for performance comfort — it is a structural
commitment that permanently complicates every cross-org read, every migration,
every backup, and the HA/leader and RLS machinery that the multi-tenancy epic
just finished hardening. The right default is "don't"; this doc exists so that
*when* a concrete capacity wall arrives, the move is a deliberate, well-scoped
build on top of the work already done (the `org_id` tenant model + the `Store`
seam) rather than a re-architecture.

What sharding *is* here: **tenant-per-shard.** Every org lives wholly on exactly
one physical Postgres (a "shard"); a routing table maps `org_id → shard`; the
`Store` seam grows a sharded implementation that picks the right per-shard pool
per request. This is the only sharding model that fits Rampart, because the data
model is already org-rooted and the analytics are intra-org — see "Why org-keyed,
not hash/range" below.

---

## When this is actually needed (read this first)

Sharding earns its complexity only past thresholds a single PG genuinely cannot
hold. Reach for the cheaper levers first, in order:

1. **Vertical scale + tuning.** A modern single PG on real hardware handles
   tens of thousands of monitors and high-cardinality telemetry. `connect()`
   defaults to 16 pool connections "fine for a homelab" (lib.rs:99) — production
   capacity is a `max_connections` and an instance-size change, not an
   architecture change.
2. **Read replicas.** Rampart's load is read-heavy on the management/dashboard
   side (heartbeat aggregates, SLO burndown, log/trace search). A streaming
   replica behind a read-only pool absorbs that with zero app changes to the
   write path. This is the highest-leverage step before sharding and should be
   exhausted first.
3. **Telemetry partitioning / tiering.** The genuinely unbounded tables are the
   *telemetry* ones — heartbeats, spans, logs, metric_samples, RUM, error
   events. Native PG declarative **partitioning** (by time, and optionally by
   `org_id` hash) plus the existing retention prune (prune.rs) and head-sampling
   (already shipped) keep one PG viable far past where naive single-table growth
   would not. Partitioning is *intra-instance* sharding and carries none of the
   cross-shard query pain below.
4. **Then, and only then, horizontal sharding** — and even then only if the
   driver is **one of**: (a) a single tenant whose telemetry alone exceeds one
   instance (split *that* org out — see "the whale" pattern), (b) a regulatory
   data-residency requirement forcing per-region placement (EU org → EU shard),
   or (c) an MSP/reseller fleet of thousands of orgs whose *aggregate* write
   volume saturates one primary's WAL/IO.

If the requirement is (b) data residency, note it is the *cleanest* sharding
case and may justify the work even at modest scale — placement, not capacity, is
the driver, and the routing table doubles as a residency ledger.

**Anti-signal:** "we might need it later" is not a driver. Building the sharded
`Store` speculatively forks every future schema migration across N shards (the
silent-drift risk from MULTI_DB §risk-2, here multiplied by shard count) and
permanently taxes cross-org admin views — for capacity you do not have a wall
for. Don't.

---

## What the existing architecture gives us (verified against the tree)

Sharding is *materially cheaper here than in most systems* because two pieces of
load-bearing groundwork already exist:

| Asset | Where | Why it matters for sharding |
|---|---|---|
| **Org-rooted data model** | `0108_org_id_columns.sql`; `org_id` on 30 tenant-root tables, children inherit via FK | The shard key already exists on every tenant row. No data-model surgery to *find* the key. |
| **`Store` seam (object-safe)** | `rampart-db/src/store.rs` — `trait Store: <46 sub-traits>`, `PgStore { pool }`, `AppState` holds `Arc<dyn Store>` (state.rs:24/119) | The sharded backend would be **a second `impl Store`** (`ShardedStore`), the same shape MULTI_DB plans for `SqliteStore` (P1-0 foundation shipped v0.156.0; full `impl Store` still in progress — today `PgStore` is the only complete impl). AppState already abstracts over `Arc<dyn Store>`, so the P0-seam payoff is reusable here. |
| **`org_id` threaded through every fn** | scoped fns take `OrgId`; system callers use `*_all`/`*_unscoped` siblings (lib.rs:8) | Every scoped call **already carries the shard key as an argument.** The router has the org without a re-plumb. The `*_unscoped` siblings are the exact set of "this query has no org → it is a cross-shard problem" call sites — they are pre-flagged. |
| **Request-boundary org resolution** | auth.rs:284-303 resolves `active_org_id` (→ Default fallback); `rls::CURRENT_ORG` task-local already carries it down (auth.rs tail, rls.rs:22) | The shard router reads the **same** `CURRENT_ORG` task-local the RLS hook reads. One org-resolution chokepoint feeds both RLS and shard routing. |
| **Per-org ingest credentials** | `ingest_keys` resolve org from the credential, never the body (MULTITENANCY P5) | The ingest tier — the highest-volume write path — already knows its org *before* it touches the DB. The ingest write path can route to the right shard with no body inspection. |

The single most important consequence: **the shard key is `org_id`, it is already
present at every call site and at the request boundary, and the seam to host the
router already exists.** Sharding is a routing-and-topology problem here, not a
data-model problem. That is the good news. The rest of this doc is the bad news.

---

## Why org-keyed (tenant-per-shard), not hash/range sharding

| Model | Verdict | Reason |
|---|---|---|
| **Tenant-per-shard (org_id → shard)** | **chosen** | All analytics are intra-org (`WHERE … m.org_id = $` joins, heartbeats.rs:732). A single org's data colocated on one shard means every dashboard/SLO/log-search query is a **single-shard** query — no scatter-gather on the hot path. Matches data residency. Natural unit of placement, backup, and "move a tenant". |
| **Hash-shard on a row PK** (spray rows across shards) | rejected | Would scatter every intra-org aggregate (`percentile_cont`, window funcs, recursive folder inheritance, the detection boolean-tree compiler) across all shards and force cross-shard joins on the hot path — exactly the analytics Rampart exists to do, made the worst case. This is the Cassandra-tier failure mode from MULTI_DB §Cassandra, self-inflicted. |
| **Range-shard on time** | rejected as the *sharding* axis | Time is the right *partitioning* axis **within** a shard (declarative partitions + retention prune), not the cross-instance routing axis. Use it intra-shard (lever #3 above), not as the shard key. |

Tenant-per-shard keeps Rampart's core value (intra-org analytics) on the
single-shard fast path. The cost it imposes is entirely on **cross-org** reads —
which are admin/global/fleet views, low-QPS by nature. We are trading hot-path
simplicity for cold-path complexity. That is the correct trade for this product.

---

## Architecture: the `ShardedStore`

A new `impl Store` living beside `PgStore`, selected at boot:

```
AppState ── Arc<dyn Store> ──┬── PgStore { pool }            (today; single-PG default)
                             └── ShardedStore {              (new; opt-in)
                                    catalog:  Arc<dyn ShardCatalog>,   // org_id → ShardId
                                    pools:    HashMap<ShardId, DbPool>,// one pool per shard
                                    control:  DbPool,                  // global/control-plane PG
                                 }
```

The dispatch rule per method:

- **Scoped methods** (take an `OrgId`, the overwhelming majority): resolve
  `shard = catalog.shard_for(org_id)`, pick `pools[shard]`, delegate to the
  existing free fn against that pool. **Byte-identical SQL** — the same
  `crate::monitors::list(pool, org_id)` call, just a different pool. This is the
  whole point of reusing the seam: the per-shard SQL is unchanged.
- **Org-implicit methods** (resolve org from the `CURRENT_ORG` task-local rather
  than an arg — the RLS/ingest path): read the task-local, route the same way.
- **Cross-shard / `*_unscoped` / `*_all` methods**: the hard part — see below.
- **Control-plane methods** (users, sessions, org_members, organizations,
  api_keys's identity half, oidc_state, the audit chain): route to `control`,
  **never** to a tenant shard — see "What stays global".

### What stays global (the control plane)

This is the design's spine and the place most naive shard designs break. Verified
against the tree: **`users` has no `org_id`** (it is a global identity table;
grep of users.rs shows no org column — the role mirrors onto the *Default-org
membership* per orgs.rs:101, the user row itself is global), **`sessions` carries
only `active_org_id`** (sessions.rs:23, not an ownership column), and
`organizations` / `org_members` are *the routing source of truth itself*.

These tables **cannot** be sharded by org, because:
- A user belongs to *many* orgs (membership many-to-many, MULTITENANCY "Membership
  model") which may live on *different* shards. There is no single org to route a
  user row to.
- `org_members` is read on **every authenticated request** (auth.rs:287
  `org_member_role`) to resolve the active-org role *before* the request knows
  which shard to talk to. It is the routing dependency; it must be globally
  reachable, ideally cached.
- `organizations` *is* the catalog's backing data (slug→id, the OIDC mapping key).

Therefore: a dedicated **control-plane Postgres** (`control` pool) holds the
global identity/auth/routing/audit tables. Tenant shards hold only the
`org_id`-rooted tables. The split is:

- **Control plane (global, one instance, low volume):** `users`, `sessions`,
  `organizations`, `org_members`, `oidc_login_state`, `recovery_codes`, the
  identity half of `api_keys` + `ingest_keys` (the hashed-token→org_id lookup
  must be global so an ingest request can *find* its shard), `settings`, and the
  **audit_log hash chain** (see "Audit chain" — it is deliberately a single
  global chain).
- **Tenant shards (org-rooted, the volume):** monitors, heartbeats, spans, logs,
  metric_samples, RUM, error events/issues/projects, incidents, detection rules
  + findings, escalations, SLOs, status pages, notifications, etc. — everything
  carrying `org_id`.

The control plane never holds telemetry, so it stays small and cacheable.
**`ingest_keys`/`api_keys` token lookup is the critical routing read on the hot
path** — it must be global (to resolve org→shard) and should be cached
aggressively (it already only does `find_by_token` + `touch_last_used`).

### The `ShardCatalog` (routing table)

```
trait ShardCatalog: Send + Sync {
    async fn shard_for(&self, org: OrgId) -> Result<ShardId, ShardError>; // hot path → cached
    async fn all_shards(&self) -> Vec<ShardId>;                           // fan-out targets
    async fn assign(&self, org: OrgId, shard: ShardId) -> ...;            // org creation / rebalance
}
```

Backing data: an `org_shards(org_id PK, shard_id, state)` table on the **control
plane**. `state` is an enum (`active | migrating | read_only`) used by
rebalancing (below). The catalog is **read on essentially every request**, so it
is an in-process cache (e.g. an `arc-swap`'d `HashMap<OrgId, ShardId>`) with
invalidation on assignment changes; a cache miss falls through to the control DB.
A new org's shard is chosen at `create_with_owner` time (orgs.rs:263) by a
placement policy (least-loaded, or residency-pinned).

Shards themselves are a static-ish config map `ShardId → connection URL` (env or
a control-plane `shards` table), so adding a shard is config + a migration run,
not a code change.

### Pool management

Today: one `connect(url, max)` pool (lib.rs:117). Sharded: **one pool per shard
plus the control pool**, each built by the existing `connect()` (the RLS
`before_acquire` hook is per-pool and composes fine — it reads the same
`CURRENT_ORG` task-local on whichever shard pool was chosen). Sizing: the
homelab default of 16/pool becomes *N shards × per-shard* — operators must size
deliberately, because total file descriptors / PG backends now multiply by shard
count. A lazy-connect option (build a shard's pool on first use) matters for
fleets with many small shards. The control pool wants its own (larger, since
every request hits it for routing + role) sizing.

### Leader election & the scheduler (advisory locks are per-DB)

This is the subtle one and it interacts with `leader.rs` directly.
`pg_try_advisory_lock(LOCK_KEY)` (leader.rs:24,74) is scoped to **one Postgres
instance.** A single global advisory lock no longer means "one scheduler in the
fleet" once data lives on N shards — and worse, the scheduler's leading tick is
`monitors::list_all(&pool)` (scheduler/lib.rs:1074), an **unscoped, single-pool**
query that on a sharded deployment would only ever see the monitors on whichever
pool it was handed.

Two coherent options; **option B is recommended**:

- **Option A — one global scheduler, fan-out reads.** Keep a single leader
  (advisory lock on the *control* plane). The scheduler's `list_all`-style ticks
  become **fan-out across all shards** (`catalog.all_shards()` → query each → merge).
  Simpler ownership (one leader), but every leader tick is now a scatter-gather
  and the leader carries the whole fleet's probe load. Acceptable at small shard
  counts; does not scale the *work*, only the *data*.
- **Option B — per-shard leadership (recommended).** Run the election loop
  **once per shard** (advisory lock acquired on *each shard's* own pool), so
  each shard elects its own scheduler-owner among the replicas connected to it,
  and that owner runs the probe/notifier/escalation/prune loops **for that
  shard's orgs only** (the per-shard `list_all` is now correct and bounded). This
  is the natural fit: advisory locks are per-DB, so *make leadership per-DB.*
  Failover, keepalive, and the `Leadership::always()` single-process path
  (leader.rs:44) all generalize to "per shard". The cost: `Leadership` becomes a
  `HashMap<ShardId, Leadership>` and the scheduler's loops iterate shards. The
  audit-chain advisory lock (audit.rs:54) lives on the control plane and is
  unaffected (one global chain — see below).

The notifier's same-org fan-out validation (MULTITENANCY: "Notifier validates
channels are same-org") stays correct for free, because same-org ⇒ same-shard.

### RLS interaction (the flag-gated defense-in-depth layer)

RLS (`RAMPART_RLS`, rls.rs / lib.rs:124) is **per-connection** and orthogonal to
sharding — and they actually *compose cleanly*, with one caveat:

- The RLS `before_acquire` hook binds `app.current_org` from `CURRENT_ORG`
  (lib.rs:149). On a sharded deploy, a connection from `pools[shard]` will only
  ever be bound to an org that *lives on that shard* (the router guarantees it),
  so the policy `org_id = app_current_org()` (0115) is satisfiable and correct
  per-shard. RLS becomes a *second* check that the router sent the request to the
  right shard — a nice belt-and-suspenders against a router bug.
- **Caveat — every shard needs the RLS DDL.** Migrations 0114-0116 (the
  `rampart_app` role, policies, `ENABLE`) must be applied to **every shard**, and
  the `rampart_app` role created on each. This folds into "migrations across
  shards" below; it is not free but it is mechanical.
- The control plane holds `users`/`org_members` which are **not** in the policied
  set (they have no `org_id`); RLS does not apply to them and must not (auth must
  read them before any org is bound). No change there.

---

## Cross-shard queries — the hard part

Tenant-per-shard makes the hot path single-shard; it makes **admin/global/fleet
views** genuinely hard. These are precisely the `*_all` / `*_unscoped` siblings
that already exist (lib.rs:8) — and that pre-existing set is the *exact inventory*
of what breaks. They fall into four buckets:

### Bucket 1 — Per-org admin views that *cross orgs only for an operator*
e.g. a fleet dashboard showing every org's monitor count / error rate; the
storage-usage view (`metrics::storage_usage`, metrics.rs:162, today a single
`pg_*` table-size query). **Approach: scatter-gather + merge in the app.** Fan out
the scoped query to `all_shards()`, run it per-shard *with* the org filter, merge
results. Bounded by shard count, low QPS, operator-facing → acceptable. Each such
view is hand-written; there is **no generic cross-shard query engine** and we
should not build one (that road is a distributed SQL database; out of scope).

### Bucket 2 — The scheduler's unscoped reads
`monitors::list_all` (scheduler/lib.rs:1074) and the per-tick reconcile. **Solved
by per-shard leadership (Option B):** the read is naturally per-shard and bounded,
not a cross-shard problem at all. This is the strongest argument for Option B.

### Bucket 3 — Global aggregates that genuinely span orgs
The instance `/metrics` Prometheus scrape (deliberately instance-global,
MULTITENANCY "leak traps"), pipeline gauges, self-metrics. **Approach: per-shard
counters exported with a `shard` label**, aggregated in Prometheus/Grafana, not
in the app. Do **not** try to sum them in-process on the hot path. Self-metrics
already run per-process; they become per-shard naturally.

### Bucket 4 — Things that assume a single linear sequence
The **audit hash chain** (audit.rs) and any `BIGSERIAL` cross-row ordering. The
audit chain takes `pg_advisory_xact_lock` + reads the single chain tip + inserts
(audit.rs:54-104), and MULTITENANCY is explicit: "the single global HMAC chain is
preserved (forking per org would break previous-hash linkage)." **Decision: the
audit chain stays on the control plane as one global chain.** It is low-volume
(management actions, not telemetry), tamper-evidence requires linearity, and
forking it per shard would destroy the security property. This is the cleanest
resolution and it is already where the design points. Cross-shard `BIGSERIAL`
collisions are avoided structurally: tenant rows are keyed by app-side UUIDs
(orgs.rs:29 `OrgId::new()`, ingest), and any per-shard `BIGSERIAL` is shard-local
and never compared across shards.

### The honest limit
There is **no** support for: a single SQL query joining two orgs on different
shards, cross-shard foreign keys, or cross-shard transactions. None of these
exist in the product's hot path today (all joins are intra-org), and we
deliberately keep it that way. If a future feature needs a true cross-org join,
it is a reporting/warehouse concern (export to an OLAP store), not a sharded-OLTP
concern.

---

## Migrations across shards

Today: `sqlx::migrate!("../../migrations")` runs the 118-file set against one
pool (lib.rs:182). Sharded reality:

- **Two migration sets, by plane.** Tenant-root DDL (the `org_id` tables, RLS
  policies 0114-0116, telemetry indexes) runs against **every shard**. Global
  DDL (users, sessions, organizations, org_members, audit_log, oidc state, the
  `org_shards`/`shards` catalog tables) runs against the **control plane only**.
  The 118 existing migrations must be classified once into these two buckets.
  This is the silent-drift risk (MULTI_DB §risk-2) made concrete: a schema change
  must land on N shards atomically-enough that the app's compile-checked queries
  (~490 `query!` macros, MULTI_DB) match every shard's schema.
- **Orchestration.** Boot-time `migrate()` becomes "migrate control, then migrate
  each shard" — but for a fleet this should move to an **operator-run migration
  step** (a `rampart migrate --all-shards` subcommand) rather than every replica
  racing to migrate every shard on startup. Online schema changes on large
  telemetry tables want `CONCURRENTLY` index builds and expand/contract patterns;
  that discipline is unchanged from single-PG, just multiplied.
- **Adding a shard** = provision PG → run the tenant migration set → register in
  the `shards` table → it becomes a placement target. No app deploy.
- **Version skew.** During a rolling schema change, shards are transiently at
  different versions. The compile-checked macros assume one schema. **Rule:**
  only ever apply *additive* (expand) migrations live, never a contracting change
  until every shard is migrated and the old code is gone — the same expand/contract
  discipline single-PG already needs, now with shard count as the blast radius.

---

## Rebalancing (moving a tenant between shards)

The "move a whale off the shared shard" and "rebalance a hot shard" operation.
Tenant-per-shard makes this *tractable* (one org = a self-contained subgraph of
`org_id`-rooted rows) but it is still the riskiest live operation. Sketch:

1. **Mark migrating.** `org_shards.state = migrating` for the org; the catalog
   starts routing that org's **writes** with awareness it is in flight.
2. **Snapshot + copy.** Dump the org's rows (all tenant-root tables `WHERE
   org_id = $` + their FK-children) from source shard, load into target.
3. **Catch-up + cutover.** Either (a) brief **read-only** window for the org
   (`state = read_only`) — copy the delta, flip `shard_id`, invalidate the
   catalog cache, set `state = active` — acceptable for a single tenant's short
   window; or (b) a CDC/dual-write scheme for zero-downtime (much more
   complexity; defer until a customer needs it).
4. **Verify + reap.** Row-count + checksum the copied subgraph, then delete from
   the source shard.

The per-org read-only flag is the pragmatic v1: one tenant briefly read-only
during their own move is a far smaller blast radius than fleet-wide. **Telemetry
in flight during the move** is the sharp edge — ingest for that org must either
buffer or be briefly rejected (the `RAMPART_MULTI_ORG` 401 path already exists as
a rejection mechanism). Automated, hands-off rebalancing is explicitly **out of
scope for v1**; this is an operator-driven, observed operation.

---

## Phased plan

Each phase is independently shippable and the early ones are valuable even if
sharding is never finished. **The whole program is owner-gated and should not
start until a concrete capacity/residency driver exists (see "When needed").**

- **P0 — Exhaust the non-sharding levers (do this regardless).** Read replicas
  for the read-heavy management path; declarative time-partitioning + the
  existing prune on the unbounded telemetry tables; `max_connections`/instance
  sizing. **This is the actual recommendation for ~all current installs** and
  buys the runway that makes P1+ unnecessary for most. Ships real capacity with
  zero sharding risk.
- **P1 — Control-plane split (no sharding yet).** Classify the 118 migrations
  into control vs tenant planes; introduce the `org_shards`/`shards` catalog
  tables + `ShardCatalog` trait with a **single-shard** impl (everything maps to
  one shard = today's behaviour, byte-identical). Prove the classification and
  the catalog without yet running a second PG. This is the analog of MULTI_DB's
  narrowed-P0 spike: structurally valuable, low-regret, reversible.
- **P2 — `ShardedStore` over 2 shards (the spike).** Implement the sharded
  `impl Store` routing scoped methods by `org_id`; stand up **two** shards; prove
  end-to-end on 2-3 representative domains (monitors, heartbeats, audit — audit
  proving the control-plane chain stays single + global) **before** wiring all
  46 sub-traits. Gate the full routing flip on this spike, exactly as MULTI_DB
  gates the full trait extraction on its 2-3-domain proof.
- **P3 — Cross-shard admin views.** Hand-write the scatter-gather merges for the
  `*_all`/`*_unscoped` fleet views (Bucket 1) and the storage/metrics aggregates
  (Bucket 3, per-shard labels). One per surface; no generic engine.
- **P4 — Per-shard leadership + scheduler.** `Leadership` per shard (Option B);
  scheduler loops iterate shards; per-shard probe/notifier/escalation/prune.
- **P5 — Rebalancing (operator-driven, read-only-window v1).** The move-a-tenant
  flow above, manual and observed. CDC/zero-downtime deferred until demanded.
- **P6 — Residency placement policy (only if driver (b)).** Region-pinned shard
  assignment at org creation; the catalog as a residency ledger.

---

## Named risks the owner must accept explicitly (not absorbed as scope)

1. **Operational surface multiplies by shard count.** N shards + 1 control = N+1
   Postgres instances to back up, monitor, patch, and migrate *in lockstep*. The
   homelab single-binary story (the SQLite tier in MULTI_DB) and the sharded
   story are opposite ends — sharding is an *enterprise-fleet-only* posture.
2. **Migration drift is now fleet-wide.** A schema change must reach every shard
   before the contracting half ships, against ~490 compile-checked macros that
   assume one schema. Mis-sequencing = runtime query failures on a lagging shard.
3. **Cross-org admin views permanently cost more.** Every fleet/global view is a
   hand-written scatter-gather; there is no generic cross-shard query engine and
   building one is out of scope (it is a distributed database). Some "show me
   everything across all orgs" features become expensive or get scoped out.
4. **The control plane is a new single point of failure / scaling bottleneck.**
   It is read on every authenticated request (routing + role) and every ingest
   (token→org). It must be HA in its own right (replica) and the catalog +
   token lookups must be cached, or it becomes the new wall.
5. **Rebalancing is genuinely risky.** Moving a live tenant's full subgraph with
   in-flight telemetry is the sharpest edge; v1 must accept a brief per-org
   read-only window and an operator in the loop. No hands-off auto-rebalance.
6. **Sequencing vs the rest of the roadmap.** This sits *on top of* the `Store`
   seam (multi-DB P0, done) and the org model (MT, done) — good. But it
   **conflicts with the multi-DB backends**: a `ShardedStore` assumes per-shard
   PG; the SQLite tier (P1) is single-file single-process. Sharding and the SQLite/
   homelab tier are mutually exclusive deployment shapes — pick the audience per
   release, don't try to ship both axes at once.
7. **You probably don't need it.** Restating risk-as-discipline: single PG +
   replicas + partitioning (P0) covers the vast majority of installs. Starting
   P1+ without a measured wall is paying all of risks 1-6 for capacity you
   haven't hit.

**Bottom line for the owner:** the data model and the `Store` seam make org-keyed
tenant-per-shard *the right shape and materially cheaper than greenfield* — the
shard key already exists on every row and at every call site, and the sharded
backend is "a second `impl Store`". But it is still a multi-instance,
fleet-operations, migrate-in-lockstep commitment whose entire cost lands on the
cross-org cold path and the ops surface. **The single highest-leverage,
lowest-regret step for essentially every current install is P0 (replicas +
partitioning), not sharding.** Do P1 (control-plane split + catalog, single-shard)
only when a concrete capacity or data-residency driver is on the table, and gate
the real routing (P2) on a 2-shard spike — exactly as MULTI_DB gates its backends
on a narrowed proof.
