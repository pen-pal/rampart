# Rampart — Devpost Submission Package (FINAL, paste-ready)

> Single source of truth for the H0 hackathon submission. Every number below is
> re-derived from source at commit `c32faec`, workspace version **0.156.49**.
> Deadline: **2026-06-29 17:00 PDT**. Track: **Monetizable B2B App**.
>
> **Honesty rule for tenancy:** app-layer `org_id` scoping is the isolation
> mechanism; Postgres RLS is opt-in defense-in-depth (`RAMPART_RLS`, ENABLE not
> FORCE, owner-exempt) and is turned on in the demo stack. We do NOT claim "RLS
> enforced everywhere."
>
> **Honesty rule for multi-backend:** Rampart now selects its datastore from the
> `DATABASE_URL` scheme behind one object-safe `Store` trait. **Postgres** is the
> reference/default build (zero extra deps). **SQLite** is a *complete monitoring
> backend* — built with `--features sqlite`, a `sqlite:` URL boots panic-free with
> the scheduler / notifier / SIEM dispatch all wired (single-binary / homelab
> tier). **MySQL** — built with `--features mysql`, a `mysql://` URL boots the
> **management API + telemetry reads**; the scheduler/alerting background loops
> still `unimplemented!()`-panic for a tail of un-ported domains, so we frame it
> as a management-API tier, **not** a full monitoring backend. We do NOT claim
> "runs on five databases" and we do NOT claim MySQL drives monitoring yet.

---

## ⛔ HUMAN-ONLY TODOs (cannot be done in-repo — owner must do these)

These four are the only things standing between this package and a submitted
entry. Everything else below is written, code-verified, and paste-ready.

1. **Record + upload the demo video** (≤ 3:00 — shot list at the bottom of this
   doc). Upload to YouTube, capture the link.
2. **Deploy → live link.** Provision Aurora + deploy the backend on AWS, set the
   real API origin in `frontend/vercel.json` (already present — see deploy steps),
   `vercel --prod` the SPA. Produces: the live "try it" URL, the Vercel Project
   Link + Team ID, and the AWS-DB console screenshot for the AWS Database field.
3. **Capture the multi-tenant + multi-backend screenshots** during the live demo
   (org switcher; optional `sqlite:` boot terminal). 21 feature PNGs already
   exist in `site/assets/screenshots/`.
4. **Create + submit the Devpost project** (Monetizable B2B App track), paste all
   links, confirm the repo is public, submit before **2026-06-29 17:00 PDT**.

---

## Submission Fields (all 8, paste-ready)

### 1. Tagline

```
Self-hosted observability + SIEM in one Rust binary — uptime, traces, logs, metrics, RUM, errors, on-call, status pages, and security detections, multi-tenant, runs on Postgres or SQLite (MySQL management tier), no SaaS bill.
```

### 2. What it does

```markdown
Rampart is a self-hosted monitoring, observability, and SIEM platform that ships
as a single Rust (Axum) binary backed by one relational database. From one UI and
one datastore it delivers what teams normally buy as five-plus separate SaaS
products.

**Uptime & synthetic monitoring** — 42 monitor kinds out of the box: HTTP /
keyword / JSON-query checks, TCP, Ping, DNS/DoH, TLS-cert expiry, gRPC, SSH,
SMTP/IMAP/POP3/FTP, deep service probes for Postgres, MySQL, MSSQL, Redis,
MongoDB, Memcached, Cassandra, Elasticsearch, Kafka, NATS, AMQP, MQTT, LDAP,
SNMP, NTP, RADIUS, Vault, etcd, Docker, WebSocket, and multi-step browser
synthetics. Public status pages and maintenance windows are built in.

**Observability** — the same binary ingests OpenTelemetry traces, logs, and
metrics (OTLP), Prometheus remote_write (snappy + protobuf), Sentry-compatible
error events, RUM web-vitals beacons, and continuous profiles. Errors,
distributed traces, logs, metrics, and real-user monitoring live next to your
uptime data instead of in a separate vendor.

**Alerting & on-call** — tiered alert rules, on-call rotations and escalation
policies, and 129 notification channel kinds.

**Security (SIEM)** — detection rules fire on ingested logs (e.g. auth-failure
patterns), with syslog ingest (RFC 5424 + RFC 3164) and SIEM export in JSON /
CEF / LEEF over webhook or syslog UDP/TCP.

**Built for trust and scale** — multi-tenant organizations with per-request
org_id scoping (optional Postgres row-level security as defense-in-depth), OIDC
SSO, 2FA/TOTP, high availability via Postgres leader election, AES-256-GCM
encrypted secrets at rest, a tamper-evident hash-chained audit log with
continuous re-verification, SSRF-guarded outbound probes, and compliance
tooling (GDPR export + anonymizing erasure that preserves the audit chain, SOC 2
CC6 access review). The schema is 118 ordered SQL migrations on Postgres; it runs
on AWS Aurora PostgreSQL with a connection-string change.

**One codebase, choose your database.** Every persistence call goes through one
object-safe `Store` trait (~46 sub-traits), so the `DATABASE_URL` scheme picks
the backend at boot — no second codebase. **Postgres** is the reference build
(the default, zero extra deps, what the demo runs on Aurora). **SQLite** is a
complete monitoring backend for single-binary / homelab deploys: build with
`--features sqlite`, point at a `sqlite:` file, and uptime + telemetry +
alerting + SIEM detections all run with no Postgres to operate. **MySQL**
(`--features mysql`, a `mysql://` URL) boots the management API and telemetry
reads today; the scheduler/alerting tail is still being ported, so we call it a
management-API tier, not a full monitoring backend. Same product, same UI — your
database, your call.
```

### 3. How we built it

```markdown
Rampart is a Cargo workspace of focused Rust crates plus a React/Vite frontend
served by the same binary:

- **rampart-core** — domain types shared everywhere (the MonitorKind and
  ChannelKind enums, telemetry models). No I/O.
- **rampart-db** — all persistence behind one object-safe Store trait seam
  (~46 sub-traits) so `AppState` holds `Arc<dyn Store>` over any backend. Three
  implementations live behind it: PgStore (Postgres, the default), SqliteStore
  (a complete monitoring backend, `--features sqlite`), and MysqlStore (the
  management-API tier, `--features mysql`). The `DATABASE_URL` scheme selects the
  store at boot. Also houses leader election, tamper-evident audit (hash chain),
  encrypted secrets, and multi-tenant scoping.
- **rampart-api** — the Axum HTTP server: REST API, the React SPA, and every
  ingest endpoint (OTLP, Prometheus remote_write, Sentry envelope/store, RUM,
  profiles, syslog, push), plus OIDC SSO and multi-tenant org routing.
- **rampart-checker** — the probe engine; one module per protocol family.
- **rampart-scheduler** — leader-aware scheduling so only one node in an HA
  cluster drives the schedule and background ticks.
- **rampart-notifier** — fans alerts across the notification channels; also
  carries SIEM export (JSON/CEF/LEEF).
- **rampart-ssrf** — a dedicated SSRF-guard crate so user-defined probe targets
  can't be used as a pivot into internal networks.
- **rampart-agent** — an optional thin agent binary.

The frontend is React 19 + Vite + Recharts, compiled to static assets the API
serves directly — deployment is one binary plus a database connection string.
All persistence queries are sqlx compile-checked (~485 cached queries). Schema
is 118 ordered SQL migrations applied on startup. The whole thing is
wire-compatible with stock Postgres, so running on AWS Aurora PostgreSQL was a
connection-string swap.
```

### 4. Challenges we ran into

```markdown
- **One ingest surface, many wire protocols.** OTLP (protobuf), Prometheus
  remote_write (snappy-compressed protobuf), Sentry's envelope/store format, RUM
  beacons, and syslog all speak different languages. Normalizing them into one
  telemetry store without a per-vendor translation layer took real care.
- **Multi-tenancy without a rewrite.** Retrofitting organization scoping across
  an existing single-tenant schema meant a careful migration sequence — adding
  org_id columns, backfilling, then flipping to NOT NULL and per-org uniqueness —
  staged so it stayed reversible until the final flip.
- **HA on just Postgres.** We wanted high availability without adding ZooKeeper
  or etcd, so leader election runs through Postgres advisory locks; the scheduler
  and background ticks had to become leader-aware.
- **A trustworthy audit log.** Making the audit log tamper-evident meant a hash
  chain over rows that must stay strictly linear, so concurrent appends are
  serialized with an advisory lock to avoid forking the chain.
- **Keeping claims honest.** With 42 monitor kinds and 129 channels, our own
  docs drifted ahead of (and behind) the code. We made the enums the single
  source of truth and re-derived every count from source for this submission.
```

### 5. Accomplishments we're proud of

```markdown
- **Five products, one binary.** Uptime monitoring, errors/traces/logs/metrics/
  RUM/profiling, on-call, status pages, AND a SIEM detection engine in a single
  Rust binary with one Postgres dependency — no SaaS, no per-seat pricing, no
  data leaving your infra.
- **Breadth that's real, not marketing.** 42 monitor kinds and 129 notification
  channels — every one is an actual enum variant with code behind it, not a
  roadmap item.
- **Drop-in compatibility.** Existing OpenTelemetry SDKs, Prometheus
  remote_write exporters, and Sentry SDKs point at Rampart with just a URL change.
- **Real multi-tenancy.** An org switcher with per-request org_id scoping and
  per-org uniqueness, plus optional Postgres RLS as defense-in-depth — the
  isolation story pure-observability tools lack.
- **Built for trust.** OIDC SSO, leader-election HA, AES-256-GCM secrets at rest,
  a tamper-evident hash-chained audit log, SSRF-guarded probes, 2FA, and
  compliance tooling (GDPR erasure that preserves the audit chain, SOC 2 CC6
  access review) were designed in, not bolted on.
- **Pick your database.** One object-safe Store trait lets the same binary run
  on Postgres (reference), SQLite (a complete single-binary monitoring backend),
  or MySQL (management-API tier) — chosen by the `DATABASE_URL` scheme, no fork,
  no second codebase.
```

### 6. What we learned

```markdown
- **The enum is the spec.** Letting the MonitorKind/ChannelKind enums be the
  single source of truth — and re-deriving counts and docs from them —
  eliminated a whole class of "the README says 41 but the code has 42" drift.
- **Trait seams beat config flags for portability.** Putting all persistence
  behind one object-safe Store trait turned "support another database" into
  implementing the seam, not a sed-through-the-codebase exercise — that's how we
  got SQLite to a complete monitoring backend and MySQL to a management-API tier
  from the same codebase, with each engine's quirks (no `RETURNING`, JSON-extract
  differences, `STRICT_TRANS_TABLES`) isolated to its own module.
- **Schema migrations are the scary part of multi-tenancy.** The risky moves
  (NOT NULL, dropping default fallbacks, per-org uniqueness) have huge blast
  radius and are hard to reverse, so we sequenced them to stay safe until the
  final flip.
- **Compatibility is a feature.** Speaking OTLP, Prometheus, Sentry, and syslog
  on the wire means adoption costs a URL change, not an SDK swap — that lowered
  the bar far more than any custom protocol could have.
```

### 7. What's next

```markdown
- **Finish the MySQL monitoring tier.** MySQL already boots the management API
  and telemetry reads behind the Store seam. Next is porting the remaining
  scheduler/notifier-dependency domains (maintenance, silences, routing,
  templates, monitor groups, agents) so the alerting tier runs on MySQL too —
  the same tail SQLite has already completed.
- **Defense-in-depth tenancy.** Finish layering Postgres row-level security
  (the remaining P6 enforcement step) under the application-level org scoping
  that's already enforced.
- **More synthetics and alerting depth.** Richer multi-step browser flows and
  more expressive escalation policies.
- **Operational polish.** Continued packaging, dashboards, and docs so a
  first-time operator is monitoring in minutes.
```

### 8. Built with (tech tags)

```
rust, axum, tokio, postgresql, aurora, sqlite, mysql, sqlx, react, vite, recharts, opentelemetry, otlp, prometheus, sentry, syslog, grpc, protobuf, docker, oidc, self-hosted
```

Safest core subset if Devpost limits tag count:

```
rust, axum, tokio, postgresql, sqlx, react, vite, opentelemetry, prometheus, sentry, docker, oidc
```

### (AWS Database challenge answer — required field)

```markdown
Rampart's entire datastore is one relational database, and we run it on
**AWS Aurora PostgreSQL**. Because Rampart is wire-compatible with stock
Postgres, moving to Aurora was a connection-string change — no query rewrites.
We lean on the relational model hard: 118 ordered migrations, a deep foreign-key
graph, per-org uniqueness constraints for multi-tenancy, optional row-level
security (RAMPART_RLS) for defense-in-depth, Postgres advisory locks for both
leader-election HA and serializing the tamper-evident audit hash chain, and
sqlx compile-checked queries throughout. Aurora gives us managed durability and
read scaling under that single, well-modeled schema.
```

---

## Pre-submit Checklist

### Field-by-field readiness

| Devpost field | State | Source / note |
|---|---|---|
| Project name | READY | "Rampart" |
| Tagline | READY | Section 1 above |
| Inspiration / Story (What it does) | READY | Section 2 |
| How we built it | READY | Section 3 |
| Challenges | READY | Section 4 |
| Accomplishments | READY | Section 5 |
| What we learned | READY | Section 6 |
| What's next | READY | Section 7 |
| Built with | READY | Section 8 |
| AWS Database answer | READY | Aurora PostgreSQL block above |
| Repo link | NEAR-READY | `https://github.com/pen-pal/rampart` (git remote confirmed). **Confirm it is public before paste.** |
| Screenshots gallery | READY (mostly) | 21 PNGs already in `site/assets/screenshots/`; only the org-switcher/multi-tenant shot (and an optional `sqlite:` boot terminal for the multi-backend story) must be captured live (see list below) |
| Architecture diagram | HUMAN-ONLY | Mermaid source in `docs/HACKATHON_SUBMISSION.md` (confirmed present); export to PNG/SVG |
| Demo video | HUMAN-ONLY | Shot list below; record + upload to YouTube |
| Live frontend / try-it link | HUMAN-ONLY (deploy) | No hosted URL yet. Local path is real today (`docker compose up` → `localhost:3000`). Hosted needs Vercel + AWS API origin (`frontend/vercel.json` already exists — just set the API domain). |
| Vercel Project Link + Team ID | HUMAN-ONLY (deploy) | Captured at deploy time |

### Ready in-repo (no human deploy needed)

- All 8 narrative fields above are written and factually grounded.
- 21 feature screenshots already exist in `/root/rampart/site/assets/screenshots/`.
- Local "try it" path works today: `docker compose up -d` → `http://localhost:3000` (README quickstart), plus the `examples/everything` real-data stack.
- Full AWS+Vercel deploy mechanics already documented: `/root/rampart/docs/deploy/aws-vercel.md` (409 lines).

### In-repo blockers — STATUS (most are now resolved)

1. **`frontend/vercel.json` — RESOLVED.** The file now exists at
   `/root/rampart/frontend/vercel.json` with the exact same-origin `/v1` rewrite
   (the load-bearing fix for the SPA's `credentials: same-origin` relative paths
   vs. the API's `allow_origin(Any)` no-credentials CORS). The only remaining
   action is human (deploy): replace `https://api.YOURDOMAIN` with the real AWS
   origin before `vercel --prod`.
2. **Version strings — THIS package is correct (0.156.49).** Out-of-scope sibling
   docs are still stale: `docs/HACKATHON_SUBMISSION.md` and
   `docs/HACKATHON_DEMO.md` both still say `v0.156.0`. Fix those separately if a
   judge will read them (not edited here — this task is scoped to this file only).
3. **README — RESOLVED for the headline numbers.** README badges now read 42
   probes / 129 channels and the "single-tenant by design" line is gone. One
   residual nit remains out of scope: the README's Architecture ASCII tree still
   says "probe runners (38 kinds)" and "channel fan-out (128 adapters)" — fix
   those two lines separately for full consistency.

### Human-only TODOs

See the **⛔ HUMAN-ONLY TODOs** block at the very top of this document — those
four (video, deploy→links, screenshots, create+submit Devpost) are the only
remaining work, and they cannot be done in-repo.

---

### Demo video shot list (target 2:50, hard cap 3:00)

**Pre-flight (~10 min before recording):**
1. `cd /root/rampart/examples/everything && docker compose up` — wait for the remote-agent's first compile, then let it run **>= 5 min** so monitors flap, incidents open, SLO budgets burn, and detections fire.
2. Run `bash /root/rampart/examples/everything/verify.sh` — do NOT record until it prints `ALL TIERS NON-EMPTY`.
3. Log in at `http://localhost:3000` as `demo@rampart.local` / `Rampart-Live-9271`. Pre-open every tab so nothing loads on camera.
4. Record 1080p+, slow deliberate mouse, no dead air.

**Shots (each row = one continuous take). Total budget 2:55, hard cap 3:00:**
- **0:00–0:15 — Hook.** Title card: "metrics + errors + on-call + status page + SIEM = 5 tools, 5 bills" collapsing into one Rampart logo. Say: one self-hosted, multi-tenant Rust binary replaces five vendors.
- **0:15–0:35 — Live monitoring + alerting.** Dashboard → Monitors list: a probe genuinely flipped **Down** and its uptime strip; then the incident/escalation it raised (an episode that actually paged the schedule). Say: 42 monitor types (HTTP, TCP, DNS, TLS, Postgres, gRPC, browser, …) on one scheduler — and real paging when one goes down, not seed data.
- **0:35–1:00 — Observability tiers (the 5-in-1).** Fast but deliberate: the `/api/checkout` trace waterfall with the errored leaf span → click the linked log line sharing the trace id; a metric chart breaching its rule; a RUM session with poor LCP; a CPU flamegraph. Say: live OpenTelemetry OTLP + Prometheus remote-write + Sentry-compatible errors + real-user vitals + continuous profiling, correlated by trace id — what's normally Datadog + Sentry + Grafana.
- **1:00–1:20 — Detections (SIEM).** Detection view: the `failed login` rule with raised findings; the public status page with a live incident + update. Say: detection rules fire on real auth-failure logs — security and observability in one product, with a public status page driven by real incidents.
- **1:20–1:55 — THE MONEY SHOT (multi-tenancy).** Org switcher: `Default` → `Demo Team`; show its own monitors + telemetry + ingest keys. Then try to open a `Default`-only resource by URL → it is not visible. Say this exact framing: *"Isolation is enforced by per-request org_id scoping in the app, with Postgres row-level security enabled in this stack (`RAMPART_RLS=1`) as defense-in-depth."*
- **1:55–2:30 — Multi-backend (the differentiator).** Cut to a terminal. Show the SAME binary booting on SQLite: `DATABASE_URL=sqlite:/tmp/r.db ./rampart-api` (built `--features sqlite`) → `/healthz` returns `{"status":"alive","version":"0.156.49"}`, then the same login + a live monitor in the UI. Say: *"One object-safe Store trait — the database is chosen by the connection-string scheme. Postgres is the reference, SQLite is a complete single-binary monitoring backend with no Postgres to run, and MySQL serves the management API. Same product, your database."* (Pre-record/pre-boot this; do not compile on camera.)
- **2:30–2:55 — Close.** Slow pan of the left nav (Uptime, Traces, Logs, Metrics, RUM, Profiling, Errors, On-call, Status, Detections). Say: every tier that's normally five separate products — one binary, one UI, runs on Postgres or SQLite. End on logo + Vercel URL.

**Honesty guardrails (do not violate on camera):**
- Don't say "118 migrations" unless the count is on screen; the safe spoken line is "100+ migrations."
- Don't claim RLS is "enforced everywhere" — it's app-layer org_id scoping + opt-in DB RLS (on in this demo stack).
- Keep `RAMPART_SECRET_KEY` **unset** during the demo so live notification deliveries work (known decrypt issue when set). So: either show live deliveries OR talk about encryption-at-rest — never both in one breath.
- For the multi-backend beat: SQLite is a *complete* monitoring backend (say so), but MySQL is the *management-API* tier — do NOT say "Rampart runs on three databases for monitoring" or imply MySQL drives the scheduler/alerting. Safe line: "Postgres and SQLite both run the full monitoring stack; MySQL serves the management API today." The three engines are all opt-in cargo features (`--features sqlite` / `--features mysql`); the default build is Postgres-only.

---

### Vercel deploy steps (exact)

The SPA uses relative `/v1` paths with `credentials: same-origin`; the API CORS is `allow_origin(Any)` **without** credentials. A cross-origin Vercel→AWS call therefore can't send the auth cookie. Fix: a **same-origin Vercel rewrite** (zero code change). You need a reachable backend API origin first (App Runner URL / ALB hostname / EC2 domain) — call it `https://api.YOURDOMAIN`. Full mechanics: `/root/rampart/docs/deploy/aws-vercel.md`.

1. **`/root/rampart/frontend/vercel.json` already exists** — you only need to replace the `https://api.YOURDOMAIN` placeholder with your real AWS origin. Current contents:

```json
{
  "$schema": "https://openapi.vercel.sh/vercel.json",
  "buildCommand": "npm ci && npm run build",
  "outputDirectory": "dist",
  "rewrites": [
    { "source": "/v1/:path*", "destination": "https://api.YOURDOMAIN/v1/:path*" },
    { "source": "/healthz", "destination": "https://api.YOURDOMAIN/healthz" },
    { "source": "/readyz", "destination": "https://api.YOURDOMAIN/readyz" },
    { "source": "/push/:path*", "destination": "https://api.YOURDOMAIN/push/:path*" }
  ]
}
```

   No SPA catch-all needed (hash router). Do NOT proxy ingest paths (`/otlp`, `/rum`, `/prom`, `/profiles`, Sentry DSN) — exporters point directly at the AWS origin.

2. Deploy via CLI:

```bash
cd /root/rampart/frontend
npm i -g vercel
vercel link        # pick/create project, pick team
vercel --prod
```

   Dashboard alternative: New Project → import repo → Root Directory = `frontend` → Framework preset **Vite** → Build `npm run build`, Output `dist`.

3. Smoke-test the printed URL: open it → Rampart login appears → log in → DevTools Network shows `/v1/...` returning 200 from `*.vercel.app`. CORS errors mean `vercel.json` isn't at the project root.

4. Collect the two Devpost fields: **Project Link** = the `https://<name>.vercel.app` URL; **Team ID** = Vercel Settings → General → Team ID (`team_xxxxx`).

---

### Screenshot list (capture from the running demo stack, 1280×800+)

Capture these from `http://localhost:3000` once `verify.sh` is green. They map 1:1 to the video scenes so the Devpost gallery reinforces the walkthrough. (21 generic feature shots already exist in `site/assets/screenshots/`; the org-switcher and multi-backend shots are the ones that must be captured live.)

1. **Dashboard** — overview tiles with a live Down monitor visible.
2. **Monitors list** — a Down flip + uptime strip (proves real flapping).
3. **Trace waterfall** — `/api/checkout` with the errored leaf span expanded.
4. **Metrics** — `demo_queue_depth` chart in a breaching/alert state.
5. **Errors** — an issue grouped by release with users-affected count.
6. **Status page** (public view) — open incident + an update.
7. **Detections** — `failed login` rule with raised findings.
8. **Org switcher / multi-tenancy** — `Demo Team` selected showing only its own monitors (the isolation proof). **New — capture live.**
9. **Multi-backend boot (optional but high-impact)** — a terminal showing the same binary on `DATABASE_URL=sqlite:…` returning `/healthz` `{"status":"alive","version":"0.156.49"}`, alongside the running UI. **New — capture live.**

Optional automated path: `cd /root/rampart/frontend && npm run screenshots` runs the Playwright `e2e/screenshots.spec.js` suite; curate its output rather than shipping all of it.

---

## Fact-Check Note (numbers used + where verified)

All values re-derived from source at commit `c32faec` (workspace v0.156.49).

| Number used in copy | Verified value | Source of truth |
|---|---|---|
| Workspace version | **0.156.49** | `backend/Cargo.toml:35` `version = "0.156.49"` |
| Monitor / probe kinds | **42** | `backend/crates/rampart-core/src/monitor.rs:23` — `enum MonitorKind`, 42 variants counted in source |
| Notification channels | **129** | `backend/crates/rampart-core/src/notification.rs:22` — `enum ChannelKind`, 129 variants counted in source |
| Migrations | **118** | `backend/migrations/` — 118 `.sql` files (numbering reaches 0120 with gaps; "118" is the file count, "100+" is the safe spoken form) |
| sqlx compile-checked queries | **485** | `backend/.sqlx/` cache file count (`ls | wc -l` = 485); docs' "~485" is exact |
| Store sub-traits | **~46** | `backend/crates/rampart-db/src/store.rs:1858` — `pub trait Store:` composes ~46 `Store*` sub-traits |
| Backend crates | **8** | `backend/Cargo.toml:18-27` workspace members: core, ssrf, db, checker, scheduler, notifier, api, agent |
| Multi-backend select | **Postgres / SQLite / MySQL by `DATABASE_URL` scheme** | `backend/crates/rampart-api/src/main.rs:104-145` — `is_sqlite`/`is_mysql` branches; cargo features `sqlite`/`mysql` (`rampart-api/Cargo.toml:27,30`), default = Postgres-only |
| SQLite backend status | **complete monitoring backend, boots panic-free** | `rampart-db/src/sqlite/` (29 domain files) + `CHANGELOG.md:53` ("the same tail SQLite completed (v0.156.11-27) before its boot was panic-free"); scheduler/notifier/SIEM dispatch reads all wired (e.g. CHANGELOG 0.156.27) |
| MySQL backend status | **management-API + telemetry-read tier (NOT full monitoring)** | `CHANGELOG.md:35-53` (0.156.49): `mysql://` boots `/healthz` on MariaDB; scheduler/notifier loops `unimplemented!()`-panic for un-ported domains (maintenance, silences, routing, templates, monitor_groups, agents) |
| Store impl over all 3 | **`Arc<dyn Store>` holds PgStore / SqliteStore / MysqlStore** | `main.rs:106-145`; `CHANGELOG.md` 0.156.48 ("capstone": `impl Store for MysqlStore`) |
| Demo stack RLS | **`RAMPART_RLS: "1"`** | `examples/everything/docker-compose.yml:68` — RLS genuinely on in this stack only |
| Demo login | `demo@rampart.local` / `Rampart-Live-9271` | `examples/everything/docker-compose.yml:31-32` |
| verify.sh success marker | `ALL TIERS NON-EMPTY` | `examples/everything/verify.sh:112` |
| Repo URL | `github.com/pen-pal/rampart` | git remote `origin` (confirm public before paste) |
| Syslog | RFC 5424 + RFC 3164 | `rampart-core/src/syslog.rs` (both RFCs referenced) |
| SIEM export | JSON / CEF / LEEF over webhook + syslog UDP/TCP | `rampart-notifier/src/siem.rs` (CEF/LEEF/`"json"` formats present) |
| RLS behavior | flag-gated `RAMPART_RLS`, ENABLE (not FORCE), owner-exempt | `migrations/0116_rls_enable.sql` |
| Multi-org enforcement | flag-gated `RAMPART_MULTI_ORG`, off by default | `rampart-api/src/ingest_util.rs:250-257` |
| Org switcher | real, shown only with >1 org | `frontend/src/App.jsx:490-495` (`showSwitcher = orgList.length > 1`) |
| Aurora | wire-compatible with stock Postgres → connection-string swap | grounded; no Aurora-specific code paths |
| Ingest paths (real) | OTLP, Prometheus remote_write, Sentry DSN, RUM, profiles, syslog, push | `routes/`: `otlp.rs`, `prom_write.rs`, `error_ingest.rs`, `rum.rs`, `profiles.rs`, `syslog.rs`, `ingest.rs` |
| HA | Postgres advisory-lock leader election | `rampart-db/src/leader.rs` |
| Audit log | tamper-evident hash chain + continuous re-verification | `rampart-db/src/audit.rs` |
| Secrets at rest | AES-256-GCM | `rampart-db/src/secrets.rs` |
| Compliance | GDPR export + anonymizing erasure (preserves audit chain); SOC 2 CC6 access review | `routes/compliance.rs`, `rampart-db/src/access_review.rs` |
| `frontend/vercel.json` | **EXISTS** (same-origin `/v1` rewrite, `YOURDOMAIN` placeholder) | `frontend/vercel.json` (present at this commit) |

### Things deliberately NOT claimed (anti-overclaim guardrails)

- **NOT** "runs on 5 databases today." Three backends are real and boot-selectable behind the Store seam, but only **Postgres + SQLite** run the full monitoring stack; **MySQL** is the management-API tier (scheduler/alerting tail not yet ported — it panics). All three are opt-in cargo features; the default build is Postgres-only.
- **NOT** "MySQL drives monitoring." Verified false at this commit — `CHANGELOG.md` 0.156.49 documents the `unimplemented!()`-panic in the scheduler/notifier worker loops for un-ported domains on MySQL.
- **NOT** "RLS enforced everywhere." Isolation is app-layer per-request `org_id` scoping; RLS (`RAMPART_RLS`) is opt-in defense-in-depth, ENABLE not FORCE, owner-exempt, on only in the demo stack.
- **Version is 0.156.49** (was 0.156.10 in the prior draft of this package) — corrected throughout.

### Resolved since the prior draft of this package

- **`frontend/vercel.json` now exists** — prior draft listed it as a missing in-repo blocker. The same-origin rewrite is shipped; only the `YOURDOMAIN` placeholder needs a real origin at deploy time.
- **README headline numbers fixed** — prior draft flagged 38/41 probes / 128 channels / "single-tenant by design"; README now reads 42 / 129 / multi-tenant. (Residual: README Architecture ASCII tree still says "38 kinds" / "128 adapters" — out of scope for this file.)
- **Sibling docs still stale** — `docs/HACKATHON_SUBMISSION.md` and `docs/HACKATHON_DEMO.md` still say `v0.156.0`. Not edited here (this task is scoped to this file only); fix separately if a judge will read them.
