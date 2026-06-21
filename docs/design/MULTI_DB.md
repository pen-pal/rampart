# Multi-DB backing store — design & phased plan

Status: **DESIGN / NOT STARTED.** Owner asked Rampart to support more than
Postgres ("Postgres/MySQL/SQLite/Mongo/Cassandra — all"). This document is the
result of a design pass (5-agent design workflow + an adversarial critique that
re-verified every load-bearing number against the tree). It is the plan of
record. **No implementation has begun**; the first real phase is owner-gated
(see "Recommendation" — it is a quarter-scale effort and conflicts in timing
with the in-flight multi-tenancy Phase 6 work).

The honest framing, up front: **Postgres is the full-feature tier; every other
engine is a capability *subset*, not parity.** RLS DB-enforcement, foreign keys,
advisory-lock HA leader election, the linear tamper-evident audit hash-chain, and
compile-checked queries do **not** all survive the trip to every engine. Promising
"runs on any of 5 databases, same features" would be dishonest. What we can
truthfully offer is: PG = reference/default/full; SQLite = single-binary/homelab
subset; MySQL = relational subset for shops with MySQL ops; Mongo/Cassandra =
from-scratch query layers, demand-gated.

---

## Why this is hard (the facts, verified against the tree)

Rampart is deeply Postgres-coupled today. Counts confirmed by the critique pass:

| Coupling | Count | Where it bites |
|---|---|---|
| Compile-checked macros (`query!`/`query_as!`/`query_scalar!`) | **480** (353/99/28) | each is per-driver; needs a concrete driver + an offline `.sqlx` cache. `sqlx::Any` cannot drive them. |
| `pool: &DbPool` fn signatures | **443** | the surface to invert behind a trait. |
| Free-fn call sites `rampart_db::mod::fn(pool, …)` | **690** across 57 files (574 `.pool()`) | the blast radius of switching to `&dyn Store`. |
| `make_interval(…)` | 51 | dialect: MySQL `DATE_SUB`, SQLite `datetime('now',±?)`. |
| `RETURNING` | 48 | MySQL has none → INSERT-then-SELECT (UUID-PK app-side softens it). |
| `ON CONFLICT` upserts | 28 | MySQL `ON DUPLICATE KEY` (loses per-constraint targeting + WHERE). |
| `percentile_cont` (ordered-set agg) | 18 | SQLite/MySQL/Mongo/Cassandra: app-side compute. |
| `.begin()` multi-statement transactions | 11 | see object-safety wall below. |
| `#[sqlx::test]` tests | **232** | the regression net — and it is PG-template-DB-specific machinery, **rebuilt per engine**, not "re-pointed". The single most underestimated cost. |
| `pg_advisory_xact_lock` (audit chain) | audit.rs:45 | no clean analog off PG. |
| `pg_try_advisory_lock` (leader) | leader.rs | HA leader election. |
| `tsvector` FTS | logs.rs only | SQLite FTS5 / MySQL FULLTEXT (different semantics). |
| `WITH RECURSIVE` | routing.rs only | folder-tree inheritance. |
| PG-specific types in **public** signatures | `IpNetwork` ×6, `sqlx::postgres::` ×4 | leak across the trait boundary; must map to std types at the PG edge. |

**Rejected: `sqlx::Any`.** It erases the *connection* type but not the SQL
*dialect*, and it cannot drive the 480 compile-checked macros (they require a
concrete driver + offline cache). Adopting it would throw away compile-time
checking at all 480 sites while still only covering PG/MySQL/SQLite. No.

---

## Architecture: the SEAM

A `Store` super-trait composed of ~40 domain traits (`StoreMonitors`,
`StoreHeartbeats`, `StoreDetection`, `StoreLeader`, …) mirroring the existing
flat modules, fronting the 443 `pool` fns. Callers (rampart-api,
rampart-scheduler) bind to `Arc<dyn Store>` and never see a driver.
`lib.rs:72 pub type DbPool=PgPool` and `DbResult` are the chokepoint to invert;
`DbError` stays the unified error and each impl maps driver errors into it.

- **Relational backends (PG/MySQL/SQLite)**: three native sqlx drivers, each
  with its **own** compile-checked macros and its **own** `.sqlx` cache,
  selected by `#[cfg(feature=…)]` per fn body — cfg-gate the *macro invocation*,
  not the fn (one `query_as!` per backend sharing the Rust struct). A thin
  internal `dialect` module hides the translatable idioms (placeholders `$1` vs
  `?`; casts `::t` vs `CAST`; intervals; upsert; RETURNING vs last-insert-id).
  The ~6 TIER-1 features (enums, RLS, advisory locks, FTS, UNNEST bulk-insert,
  generated cols) get per-backend **re-architecture**, not dialect strings.
- **Non-relational (Mongo/Cassandra)**: separate from-scratch crates
  (`rampart-store-mongo`, `rampart-store-cassandra`) implementing the same domain
  traits with native clients. No sqlx, no SQL, **no compile-time typing**.

### Cross-cutting abstractions (pulled out as their own traits)
- **LeaderElector** — PG `pg_advisory_lock`; MySQL `GET_LOCK`; SQLite/single-proc
  `Leadership::always()` (already exists, leader.rs:44); Mongo TTL-doc lease;
  Cassandra LWT+TTL lease.
- **TenantGuard** (the RLS connection hook, lib.rs:119-173 `SET ROLE`/`set_config`)
  — PG-only impl; everyone else installs a **no-op** (RLS is OFF-by-default
  defense-in-depth; the primary `org_id` WHERE filter is already threaded through
  every fn, so no functional regression *today* — but see risk #5).
- **Secrets** (secrets.rs AES-256-GCM) — engine-agnostic app-layer crypto; moves
  **above** the store boundary, ports verbatim.
- **AuditChain** (audit.rs:45, advisory-xact-lock + read-tip + insert + update in
  one tx) — abstract as an append-serialization concern. PG xact-lock; Mongo
  single-doc atomic + retry; Cassandra needs full redesign. This is a
  security-relevant guarantee (tamper-evidence) being downgraded off PG → an
  **explicit owner decision**, not absorbed scope.
- **Migrations** — replace `sqlx::migrate!` with a per-backend DDL set; the 115
  PG DDL files are not portable as-is.

---

## Per-engine feasibility & effort

| Engine | Feasibility | Effort | Notes |
|---|---|---|---|
| **PostgreSQL** | incumbent — refactor, not port | **3-5 wk** (but see object-safety wall) | all 480 macros + RLS + advisory locks + FTS + UNNEST + analytics stay verbatim. |
| **SQLite** | feasible; best single-binary/homelab fit | **10-16 wk** | enums→TEXT+CHECK, UNNEST→multi-VALUES, FTS5, intervals→datetime(), percentile app-side, leader=always(), RLS dropped, ctid→rowid. Keeps compile-checking (own cache). |
| **MySQL** | feasible, *harder* than SQLite | **14-20 wk** | no RETURNING (48 rewrites), ON DUPLICATE KEY, inline enums, **no array binds** (UNNEST/=ANY/array cols), FULLTEXT, DATE_SUB, GET_LOCK (session not xact → audit-chain redesign), percentile app-side. Don't lead with it on "enterprise" intuition. |
| **MongoDB** | from-scratch query layer | **5-8 mo** | doc model is a natural fit for config/attribute blobs; ~60-65% CRUD ports cleanly; the 35-40% analytics → aggregation pipelines ($setWindowFields/$percentile/$graphLookup/$dateTrunc/$lookup). **Compile-checking entirely lost.** FKs lost. Build only on concrete demand. |
| **Cassandra/Scylla** | largely **infeasible** for the analytics tier | **9-14 mo** + permanent degradation | no joins/subqueries/cross-partition aggregates/multi-row ACID. Analytics must become write-path rollups; audit-chain/detection-tree/recursive-inheritance permanently degraded. **Recommend decline** unless a concrete write-heavy scale requirement forces it. |

---

## Phased plan

- **P0 — Trait extraction (PG-only, no new backend).** Define the seam, make the
  current PG code the first impl with **zero SQL changes**, switch
  api/scheduler to `Arc<dyn Store>`, pull out LeaderElector/TenantGuard/
  AuditChain/secrets. Keep all 480 macros + 232 tests green. Ships nothing
  user-visible but unblocks everything and is valuable cleanup on its own.
  **Caveat (from critique): this is NOT zero-behavior-change** — see the
  object-safety wall.
- **P1 — SQLite** (cheapest real second backend; proves the seam isn't secretly
  PG-shaped; serves single-binary/homelab/embedded).
- **P2 — MySQL** (second relational impl reusing the dialect module).
- **P3 — MongoDB** (first non-relational; demand-gated; 5-8 mo).
- **P4 — Cassandra** (defer / likely decline).

### The object-safety wall (why P0 is not free)
A `&dyn Store` super-trait of ~40 async traits **cannot** expose the existing
borrowed `&mut Transaction<'_, Postgres>` (orgs.rs:195 `upsert_member_tx`) or the
11 in-fn multi-statement tx bodies — async-trait + dyn + a non-erasable
lifetime-bearing concrete tx handle is the classic object-safety wall. **Before
any trait**, the 11 `.begin()` sites + the cross-fn tx must be redesigned into
self-contained transactional methods that own their whole transaction internally
(e.g. one `create_user_with_default_membership` replacing tx-threading from
`users::create` into `orgs`). This is good hygiene regardless and testable
against the existing 232 tests with zero behavior change — but it is real work,
not "cleanup".

---

## Recommendation (owner-gated)

1. **DO FIRST (when greenlit): a *narrowed* P0 on Postgres only.** Reject the
   "risk-free cleanup" framing. Concretely, in order:
   1. Resolve the transaction problem first as a stand-alone pure-PG refactor
      (de-thread the 11 `.begin()` bodies + `upsert_member_tx` into tx-owning
      fns). Removes the only thing that blocks object-safety later; good hygiene
      regardless; zero behavior change against the 232 tests.
   2. Lift the already-engine-agnostic pieces out: secrets crypto (verbatim),
      and define LeaderElector + TenantGuard as traits with **only** the PG impl.
   3. Introduce the `Store` seam **incrementally behind the existing free-fn
      API** (default PG impl delegates to the current free fns; AppState holds
      `Arc<dyn Store>` while `state.pool()` still exists during transition).
      Prove object-safety end-to-end on **2-3 representative domains**
      (monitors, heartbeats, audit — including the redesigned tx methods and the
      `IpNetwork`→std-type mapping) **before** extracting all ~40 traits.
   4. Gate the full ~40-trait extraction + 690-callsite flip on that spike
      succeeding, budgeted realistically against the 232-test harness and the
      in-flight MT Phase 6 merge surface.
2. **THEN, only after the spike proves the seam AND a concrete user/requirement
   exists: P1 SQLite.** Do not start any non-PG backend on intuition.
3. **DEFER P3 Mongo; HOLD/decline P4 Cassandra.** There is currently **no demand
   signal**; even P2 MySQL is speculative until a user asks.

### Named risks the owner must accept explicitly (not absorbed as scope)
1. **Test cost ~2× understated** — 232 `#[sqlx::test]`, and the per-test
   template-DB machinery is PG-only → a from-scratch fixture framework per
   non-PG engine. Dominant P1/P2 cost.
2. **Maintenance/CI matrix explosion** — N relational backends = N `.sqlx`
   caches regenerated against live engines in CI + cfg-gated dual macro bodies
   (2-3× SQL maintenance for *every* future schema change) + 115 migrations
   forked per engine (silent drift risk). The non-PG backends rot without
   committed CI + a real user.
3. **Audit-chain linearity downgraded off PG** — security-relevant
   (tamper-evidence); explicit owner sign-off required.
4. **detection.rs QueryBuilder** is a recursive boolean-tree→SQL compiler (~66
   `qb.` calls / 3 sites), not "12 lines" — a real per-dialect subproject.
5. **RLS timing conflict** — the MT epic is moving *toward* turning RLS on (P6
   enforcement flip pending). Building SQLite/MySQL backends that structurally
   cannot enforce RLS hard-forks the security model exactly as tenant isolation
   is hardening. Sequence deliberately.
6. **Opportunity cost** — an observability product's core value is precisely the
   analytics (percentile/window/recursive/joins) that Mongo/Cassandra do worst.
   Mongo (5-8 mo) and Cassandra (9-14 mo) are quarters-to-years of from-scratch
   work with no current demand signal.

**Bottom line for the owner:** "all five" is feasible only as *tiers, not
parity*, and realistically as a multi-quarter program. The single highest-leverage,
independently-valuable, low-regret step is the narrowed P0 spike (tx de-threading
+ secrets/leader lift + 2-3-domain seam proof) on Postgres alone — and even that
should wait until MT Phase 6 settles to avoid a quarter-long merge-conflict war.
