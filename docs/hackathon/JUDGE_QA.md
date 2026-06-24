# Rampart — Anticipated Judge Q&A

> Honest, code-grounded answers to the questions a technical judge or skeptic is
> most likely to ask. Every claim points at code or config that exists today at
> workspace version **v0.157.12**. Paired docs:
> [`SUBMISSION.md`](SUBMISSION.md) (paste-ready copy),
> [`DEMO_SCRIPT.md`](DEMO_SCRIPT.md) (video shot list),
> [`GO_LIVE.md`](GO_LIVE.md) (deploy runbook),
> [`CHECKLIST.md`](CHECKLIST.md) (field readiness).
>
> Rule for this doc: if we can't point at a file, we don't claim it.

---

## 1. Is this really one binary, or a stitched-together stack?

One binary. The Cargo workspace builds a single `rampart-api` server crate that
owns the REST API, every ingest listener (OTLP, Prometheus remote-write, Sentry
DSN, RUM, profiles, syslog, push), the scheduler, the notifier, and the React
SPA. The SPA is compiled into the binary via `rust-embed`
(`backend/crates/rampart-api/src/static_assets.rs`), so there is no separate
web-server process. The only external dependency at runtime is the database;
`docker compose up -d` starts the one container plus Postgres.

## 2. How big is it / what does it actually need to run?

It needs one process and one relational database — nothing else (no ZooKeeper,
etcd, Redis, or message broker). High availability is done with **Postgres
advisory locks** for leader election (`backend/crates/rampart-db/src/leader.rs`:
`SELECT pg_try_advisory_lock(...)`), so multiple replicas coordinate through the
database you already have. The deployed container runs hardened — `cap_drop:
ALL` (keeping only `NET_RAW` for ICMP), `no-new-privileges`, and a read-only
root filesystem (`deploy/compose.aws.yaml`). Migrations apply on boot, so a
fresh database becomes a working install with no manual schema step.

## 3. Does it actually run on multiple databases, or is that a slide?

It runs on Postgres and SQLite today as **complete monitoring backends**, and on
MySQL as a **management-API tier**. Every persistence call goes through one
object-safe `Store` seam (`backend/crates/rampart-db/src/store.rs`) with three
concrete implementations — `PgStore`, `SqliteStore`
(`backend/crates/rampart-db/src/sqlite/store.rs`), and `MysqlStore`
(`backend/crates/rampart-db/src/mysql/store.rs`) — selected by the
`DATABASE_URL` scheme at boot. SQLite and MySQL are opt-in cargo features
(`sqlite`, `mysql`), off by default; the reference build is Postgres-only. We
deliberately do **not** claim "runs on five databases" or "MySQL drives
monitoring": MySQL boots the management API and telemetry reads, but the
scheduler/alerting tail for a few domains isn't ported yet (see
[`SUBMISSION.md`](SUBMISSION.md) → What's next).

## 4. Is the multi-tenancy real, or a flag on a single-user tool?

Real, and shipped across Phases 1–5: orgs, org members, per-org RBAC, an org
switcher, OIDC→org claim mapping, and per-org ingest credentials. Isolation is
enforced by threading an `OrgId` through the repository layer on every tenant
read and write. We hardened it pre-submission by closing actual cross-tenant
leaks — the live heartbeat SSE stream
(`backend/crates/rampart-api/src/routes/stream.rs`) and scheduled uptime reports
were both leaking other orgs' data; each fix shipped with a regression test
(`drops_foreign_org_heartbeats`, `render_is_org_scoped`). The demo's "money
shot" (Scene 7 in [`DEMO_SCRIPT.md`](DEMO_SCRIPT.md)) shows two live tenants and
a cross-org URL coming back not-visible.

## 5. Is RLS "enforced everywhere"? What's the honest scope?

No — and we won't say it is. App-layer per-request `org_id` scoping is the
primary isolation; Postgres row-level security is **opt-in defense-in-depth**,
gated behind `RAMPART_RLS`. The migration uses `ENABLE ROW LEVEL SECURITY`, not
`FORCE` (`backend/migrations/0116_rls_enable.sql`), and the app role is the table
owner, so it is exempt under ENABLE — which is why system loops (scheduler,
prune, notifier) keep working without an org bound. It's turned on in the demo
stack (`RAMPART_RLS: "1"` in `examples/everything/docker-compose.yml`).
Promoting RLS from defense-in-depth to the enforced default is the Phase 6
roadmap item, not a shipped claim.

## 6. OTLP / Prometheus / Sentry — is it actually drop-in, or a custom protocol?

Drop-in: point an existing exporter at Rampart with a URL change, no SDK swap.
OTLP/HTTP traces and logs (JSON + protobuf) land at `/otlp/v1/traces` and
`/otlp/v1/logs` (`backend/crates/rampart-api/src/routes/otlp.rs`). Prometheus
`remote_write` (snappy-compressed protobuf `WriteRequest`) lands at `/prom/write`
(`backend/crates/rampart-api/src/routes/prom_write.rs`). Sentry SDKs work by
pointing their DSN at Rampart — `/api/{project_id}/envelope/` (modern) and
`/api/{project_id}/store/` (legacy) in
`backend/crates/rampart-api/src/routes/error_ingest.rs`. RUM, pprof/OTLP
profiles, and RFC 5424/3164 syslog have their own real ingest routes too
(`routes/rum.rs`, `routes/profiles.rs`, `routes/syslog.rs`).

## 7. What's the security story?

It was designed in, not bolted on. A tamper-evident audit log uses a chained
hash (`backend/crates/rampart-db/src/audit.rs`) — HMAC-SHA256 when
`RAMPART_SECRET_KEY` is set, serialized by a Postgres advisory transaction lock
so the chain stays linear; the scheduler re-walks it on a slow tick and raises
both a high-severity log and a forward `audit.chain_verify_failed` event if a row
was edited, deleted, or reordered
(`backend/crates/rampart-scheduler/src/lib.rs` → `check_audit_chain`). A
dedicated SSRF guard (`backend/crates/rampart-ssrf`) blocks cloud-metadata
(`169.254.169.254`) and internal ranges on every outbound probe and webhook.
Secrets-at-rest use AES-256-GCM with a fail-closed startup check that refuses to
boot on a weak key (`backend/crates/rampart-db/src/secrets.rs`); TOTP 2FA
(`routes/totp.rs`) and OIDC SSO with alg-confusion defense (`routes/oidc.rs`)
round it out.

## 8. The "42 monitor kinds / 129 channels" headline — real or padded?

Real. Both are enums that are the single source of truth, and we recounted them
for this submission. `MonitorKind` has exactly **42** variants
(`backend/crates/rampart-core/src/monitor.rs`) — HTTP/keyword/JSON, TCP/ping/DNS,
TLS-expiry, gRPC, deep service probes (Postgres, MySQL, Redis, Mongo, Kafka,
etc.), down to headless-browser synthetics. `ChannelKind` has exactly **129**
variants (`backend/crates/rampart-core/src/notification.rs`), each with an
adapter behind it. The `examples/everything` provisioner instantiates monitors of
all 42 kinds and ~128 channels as live config (a few vendor-only kinds are
config-only on older images), and `verify.sh` asserts every telemetry tier is
non-empty.

## 9. Why Aurora PostgreSQL specifically?

Because the depth is in the **schema**, not a driver. Rampart's data model is
deliberately relational — an org-scoped foreign-key graph, composite per-org
uniqueness constraints, transactional alert routing, and time-series retention
pruning — which is exactly a relational-engine fit and the opposite of a
key-value fit (so no DynamoDB / Aurora DSQL). Aurora PostgreSQL is
wire-compatible with stock Postgres and we use `sqlx`, which reads TLS settings
straight from the connection URL, so moving onto Aurora was a connection-string
change (`?sslmode=require`), not a rewrite. We also lean on Postgres advisory
locks for both leader-election HA and serializing the audit hash chain — Aurora
gives us those plus Serverless v2 auto-scaling for a write-heavy,
retention-bound workload.

## 10. How does it scale / how is HA done without a coordinator?

Horizontal replicas coordinate purely through Postgres. Leader election is a
session advisory lock (`backend/crates/rampart-db/src/leader.rs`): exactly one
node owns the scheduler and background ticks, the rest serve the API, and failover
is automatic when the lock-holder dies — no ZooKeeper or etcd to operate. The
scheduler and slow-tick loops are leader-aware, so you never get duplicate probes
or duplicate alerts across replicas. On the data plane, Aurora Serverless v2
scales the cluster, and Aurora read-replica routing (point reads at a reader
endpoint) is the next query-tier step on the roadmap.

## 11. What's NOT done yet?

We keep this list honest on camera. (1) MySQL is a management-API tier — the
scheduler/notifier-dependency domains (maintenance, silences, routing, templates,
monitor groups, agents) aren't ported to it yet, so MySQL does not drive
monitoring. (2) RLS is opt-in defense-in-depth, not the enforced default (Phase 6
flip). (3) Aurora read-replica read/write split is roadmap, not shipped. (4)
There's a known decrypt quirk on the published image's monitor-flip notification
path when `RAMPART_SECRET_KEY` is set — so the demo runs with the key unset and
we show live deliveries *or* encryption-at-rest, never both in one breath
(see [`DEMO_SCRIPT.md`](DEMO_SCRIPT.md) honesty rule).

## 12. Is the demo real data or seeded rows?

Real data. The `examples/everything` stack brings up an instrumented Node app
emitting live OTLP traces/logs/metrics, folded CPU profiles, `@sentry/node`
errors, and browser RUM; Prometheus scrapes and `remote_write`s; Alertmanager
opens and closes incidents through the real ingest webhook; crons push real
metrics and push-monitor heartbeats; a from-source remote agent probes a
private-only target; and two isolated orgs run side by side. `verify.sh` gates on
`✅ ALL TIERS NON-EMPTY` before recording. A separate idempotent `seed-demo`
subcommand exists for populating a deployed dashboard with `[demo]`-tagged rows
when you don't want the full live stack.

## 13. Licensing / monetization — what's the business?

The pitch is self-hosted-first: one binary replaces a five-plus-product SaaS
stack (Datadog + Sentry + PagerDuty + a status-page vendor + a SIEM), with no
per-seat pricing and no telemetry leaving the customer's infrastructure — a clear
cost and data-sovereignty story for the Monetizable B2B track. Because tenancy is
genuine (per-org RBAC, ingest credentials, isolation), the same binary supports an
agency/MSP running many client orgs from one install, and the roadmap calls out a
**hosted multi-org SaaS built on the same binary** plus per-tenant retention tiers
as the paid path (see [`SUBMISSION.md`](SUBMISSION.md) → What's next). We're not
overstating a license model that isn't finalized — the monetization is the
self-host-vs-SaaS-stack value plus a hosted offering on the same codebase.

## 14. Did you build this for the hackathon, or bolt a hackathon onto an existing project?

The product predates the hackathon; the hackathon-specific work is the AWS/Aurora
deployment path, the Vercel frontend wiring, the `examples/everything` real-data
demo stack, and a focused pre-submission **hardening pass** that fixed the failure
modes that bite multi-tenant internet-facing services — cross-tenant telemetry
leaks, a public-surface password-hash DoS, and a non-Postgres crash path — each
with a named regression test (`CHANGELOG.md` v0.156.84–v0.157.12). We're explicit
about that split rather than claiming it was all built in a weekend.

## 15. What happens if a judge points an exporter at it live and nothing shows up?

The most common cause is the rewrite/origin split, which is documented. The SPA
calls the API same-origin (`fetch('/v1/...')`, `credentials: 'same-origin'`) and
CORS intentionally does **not** send `Access-Control-Allow-Credentials`, so the
fix is the `frontend/vercel.json` same-origin `/v1` rewrite — but **ingest
endpoints point directly at the AWS API origin, never through Vercel**
([`GO_LIVE.md`](GO_LIVE.md) §5). `/readyz` returns 200 only when the DB pool can
serve a query, so it's a real end-to-end liveness gate, and
`select count(*) from _sqlx_migrations` proves the schema is live in Aurora.

---

## Known limitations we'll own on camera

- **MySQL is a management-API tier, not a monitoring backend.** Postgres and
  SQLite run the full stack; MySQL's scheduler/alerting tail is still being
  ported.
- **RLS is opt-in defense-in-depth (ENABLE, not FORCE), on only in the demo
  stack** — app-layer `org_id` scoping is the primary isolation; the enforced-RLS
  flip is Phase 6.
- **Secrets-at-rest vs. live deliveries is an either/or in the demo.** The
  published image has a monitor-flip decrypt quirk when `RAMPART_SECRET_KEY` is
  set, so the demo runs with the key unset.
- **Headless-browser synthetics need an external renderer.** We don't ship a
  Chromium binary in the image; the `browser` kind points at an external headless
  service via config.
- **Aurora read-replica routing isn't wired yet** — reads and writes both hit the
  writer endpoint today.
- **Migration count is 118 and the "drop-in" claim covers OTLP / Prometheus /
  Sentry / syslog** — not every observability vendor's proprietary protocol.
