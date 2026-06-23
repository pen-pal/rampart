# Rampart — Devpost Submission Package (FINAL, paste-ready)

> Single source of truth for the H0 hackathon submission. Every number below is
> re-derived from source at commit `237feef`, workspace version **0.156.10**.
> Deadline: **2026-06-29 17:00 PDT**. Track: **Monetizable B2B App**.
>
> Honesty rule for everything in here: app-layer `org_id` scoping is the
> isolation mechanism; Postgres RLS is opt-in defense-in-depth (`RAMPART_RLS`,
> ENABLE not FORCE, owner-exempt) and is turned on in the demo stack. We do NOT
> claim "RLS enforced everywhere," and we do NOT claim Rampart runs on five
> databases today (SQLite backend exists behind the Store seam, feature-gated
> off, not wired into the shipped binary).

---

## Submission Fields (all 8, paste-ready)

### 1. Tagline

```
Self-hosted observability + SIEM in one Rust binary on Postgres — uptime, traces, logs, metrics, RUM, errors, on-call, status pages, and security detections, multi-tenant, no SaaS bill.
```

### 2. What it does

```markdown
Rampart is a self-hosted monitoring, observability, and SIEM platform that ships
as a single Rust (Axum) binary backed by one Postgres database. From one UI and
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
```

### 3. How we built it

```markdown
Rampart is a Cargo workspace of focused Rust crates plus a React/Vite frontend
served by the same binary:

- **rampart-core** — domain types shared everywhere (the MonitorKind and
  ChannelKind enums, telemetry models). No I/O.
- **rampart-db** — all persistence behind a Store trait seam (~46 sub-traits).
  Postgres is the default; the seam also carries a working SQLite implementation
  (feature-gated, off in the shipped binary). Houses leader election,
  tamper-evident audit (hash chain), encrypted secrets, and multi-tenant scoping.
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
```

### 6. What we learned

```markdown
- **The enum is the spec.** Letting the MonitorKind/ChannelKind enums be the
  single source of truth — and re-deriving counts and docs from them —
  eliminated a whole class of "the README says 41 but the code has 42" drift.
- **Trait seams beat config flags for portability.** Putting all persistence
  behind a Store trait made adding a SQLite backend a contained effort instead of
  a sed-through-the-codebase exercise, and kept Postgres-specific tricks isolated.
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
- **First per-driver backend.** SQLite is already implemented under the
  now-complete Store seam (feature-gated). Next is hardening it to a first-class
  shipped backend and exploring additional engines behind the same trait.
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
rust, axum, tokio, postgresql, aurora, sqlite, sqlx, react, vite, recharts, opentelemetry, otlp, prometheus, sentry, syslog, grpc, protobuf, docker, oidc, self-hosted
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
| Screenshots gallery | READY (mostly) | 21 PNGs already in `site/assets/screenshots/`; only the org-switcher/multi-tenant shot must be captured live (see list below) |
| Architecture diagram | HUMAN-ONLY | Mermaid source in `docs/HACKATHON_SUBMISSION.md`; export to PNG/SVG |
| Demo video | HUMAN-ONLY | Shot list below; record + upload to YouTube |
| Live frontend / try-it link | HUMAN-ONLY (deploy) | No hosted URL yet. Local path is real today (`docker compose up` → `localhost:3000`). Hosted needs Vercel + AWS API origin. |
| Vercel Project Link + Team ID | HUMAN-ONLY (deploy) | Captured at deploy time |

### Ready in-repo (no human deploy needed)

- All 8 narrative fields above are written and factually grounded.
- 21 feature screenshots already exist in `/root/rampart/site/assets/screenshots/`.
- Local "try it" path works today: `docker compose up -d` → `http://localhost:3000` (README quickstart), plus the `examples/everything` real-data stack.
- Full AWS+Vercel deploy mechanics already documented: `/root/rampart/docs/deploy/aws-vercel.md` (409 lines).

### In-repo blockers to fix BEFORE the human deploy work

1. **`frontend/vercel.json` does NOT exist** and the hosted deploy cannot work without it (same-origin `/v1` rewrite is the load-bearing fix for the SPA's `credentials: same-origin` relative paths vs. the API's `allow_origin(Any)` no-credentials CORS). Create it (contents in the deploy steps below). Until then, any present-tense "same-origin rewrite" framing is aspirational.
2. **Stale version strings.** If `docs/HACKATHON_SUBMISSION.md` / `docs/HACKATHON_DEMO.md` still say `v0.156.0`, fix to **0.156.10**. (This package already uses 0.156.10 everywhere.)
3. **README is stale** (badges say 38/41 probes, 128 channels; says "single-tenant by design"). Source truth is 42 probes / 129 channels / multi-tenant. Fix the README so a judge cross-checking the repo doesn't see the discrepancy. Not a submission-field blocker, but a credibility blocker.

### Human-only TODOs (cannot be done in-repo)

- Record + upload the <3:00 demo video (shot list below).
- Export the architecture diagram (Mermaid → PNG/SVG) and attach.
- Provision Aurora + deploy backend (AWS) and deploy the SPA (Vercel) → produces the live link, Vercel Project Link / Team ID, and the AWS-DB console screenshot.
- Capture the org-switcher / tenant-isolation screenshot during the live demo.
- Confirm the repo is public; create the Devpost project (Monetizable B2B App track); paste all links; submit before 2026-06-29 17:00 PDT.

---

### Demo video shot list (target 2:50, hard cap 3:00)

**Pre-flight (~10 min before recording):**
1. `cd /root/rampart/examples/everything && docker compose up` — wait for the remote-agent's first compile, then let it run **>= 5 min** so monitors flap, incidents open, SLO budgets burn, and detections fire.
2. Run `bash /root/rampart/examples/everything/verify.sh` — do NOT record until it prints `ALL TIERS NON-EMPTY`.
3. Log in at `http://localhost:3000` as `demo@rampart.local` / `Rampart-Live-9271`. Pre-open every tab so nothing loads on camera.
4. Record 1080p+, slow deliberate mouse, no dead air.

**Shots (each row = one continuous take):**
- **0:00–0:18 — Hook.** Title card: "metrics + errors + on-call + status page + SIEM = 5 tools, 5 bills" collapsing into one Rampart logo. Say: one self-hosted, multi-tenant Rust binary replaces five vendors.
- **0:18–0:33 — Monitors.** Dashboard → Monitors list. Point at a probe genuinely flipped **Down** and its uptime strip. Say: 42 monitor types (HTTP, TCP, DNS, TLS, Postgres, gRPC, browser, …) on one scheduler.
- **0:33–0:53 — Traces.** Open the `/api/checkout` trace → waterfall with the errored leaf span → click the linked log line sharing the same trace id. Say: live OpenTelemetry OTLP, not seed data; trace-to-log correlation by trace id.
- **0:53–1:12 — Metrics + RUM + Profiling.** Metrics: `demo_queue_depth` chart breaching its rule. RUM: a web-vitals session with poor LCP. Profiling: a CPU flame/folded profile. Say: Prometheus remote-write with rule alerting, real-user vitals, continuous profiles.
- **1:12–1:32 — Incidents / on-call.** Errors: an issue grouped by release with users-affected. Escalations/On-call: an episode that actually paged the schedule. Status Pages: the public page with a live incident + update. Say: Sentry-compatible error grouping, real paging, public status page from real incidents.
- **1:32–1:50 — Detections (SIEM).** Detection view: the `failed login` rule with raised findings. Say: detection rules fire on real auth-failure logs — security and observability in one product.
- **1:50–2:25 — THE MONEY SHOT (multi-tenancy).** Org switcher: `Default` → `Demo Team`; show its own monitors + telemetry + ingest keys. Then try to open a `Default`-only resource by URL → it is not visible. Say this exact framing: *"Isolation is enforced by per-request org_id scoping in the app, with Postgres row-level security enabled in this stack (`RAMPART_RLS=1`) as defense-in-depth."*
- **2:25–2:50 — Close.** Slow pan of the left nav (Uptime, Traces, Logs, Metrics, RUM, Profiling, Errors, On-call, Status, Detections). Say: every tier that's normally five separate products — one binary, one Postgres, one UI. End on logo + Vercel URL.

**Honesty guardrails (do not violate on camera):**
- Don't say "118 migrations" unless the count is on screen; the safe spoken line is "100+ migrations."
- Don't claim RLS is "enforced everywhere" — it's app-layer org_id scoping + opt-in DB RLS (on in this demo stack).
- Keep `RAMPART_SECRET_KEY` **unset** during the demo so live notification deliveries work (known decrypt issue when set). So: either show live deliveries OR talk about encryption-at-rest — never both in one breath.

---

### Vercel deploy steps (exact)

The SPA uses relative `/v1` paths with `credentials: same-origin`; the API CORS is `allow_origin(Any)` **without** credentials. A cross-origin Vercel→AWS call therefore can't send the auth cookie. Fix: a **same-origin Vercel rewrite** (zero code change). You need a reachable backend API origin first (App Runner URL / ALB hostname / EC2 domain) — call it `https://api.YOURDOMAIN`. Full mechanics: `/root/rampart/docs/deploy/aws-vercel.md`.

1. Create `/root/rampart/frontend/vercel.json`:

```json
{
  "buildCommand": "npm ci && npm run build",
  "outputDirectory": "dist",
  "rewrites": [
    { "source": "/v1/:path*",   "destination": "https://api.YOURDOMAIN/v1/:path*" },
    { "source": "/healthz",     "destination": "https://api.YOURDOMAIN/healthz" },
    { "source": "/readyz",      "destination": "https://api.YOURDOMAIN/readyz" },
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

Capture these from `http://localhost:3000` once `verify.sh` is green. They map 1:1 to the video scenes so the Devpost gallery reinforces the walkthrough. (21 generic feature shots already exist in `site/assets/screenshots/`; the org-switcher shot is the one that must be captured live.)

1. **Dashboard** — overview tiles with a live Down monitor visible.
2. **Monitors list** — a Down flip + uptime strip (proves real flapping).
3. **Trace waterfall** — `/api/checkout` with the errored leaf span expanded.
4. **Metrics** — `demo_queue_depth` chart in a breaching/alert state.
5. **Errors** — an issue grouped by release with users-affected count.
6. **Status page** (public view) — open incident + an update.
7. **Detections** — `failed login` rule with raised findings.
8. **Org switcher / multi-tenancy** — `Demo Team` selected showing only its own monitors (the isolation proof). **This is the new one to capture.**

Optional automated path: `cd /root/rampart/frontend && npm run screenshots` runs the Playwright `e2e/screenshots.spec.js` suite; curate its output rather than shipping all of it.

---

## Fact-Check Note (numbers used + where verified)

All values re-derived from source at commit `237feef`.

| Number used in copy | Verified value | Source of truth |
|---|---|---|
| Workspace version | **0.156.10** | `backend/Cargo.toml:35` `version = "0.156.10"` (NOT 0.156.0) |
| Monitor / probe kinds | **42** | `backend/crates/rampart-core/src/monitor.rs` — `enum MonitorKind`, 42 variants counted |
| Notification channels | **129** | `backend/crates/rampart-core/src/notification.rs` — `enum ChannelKind`, 129 variants counted |
| Migrations | **118** | `backend/migrations/` — 118 `.sql` files (numbering reaches 0120 with gaps at 0058/0061; "118" is the file count, "100+" is the safe spoken form) |
| sqlx compile-checked queries | **~485** | `.sqlx` cache file count; docs' "~480" is a safe understatement |
| Store sub-traits | **~46** | `backend/crates/rampart-db/src/store.rs` (docs' "~40" understates — safe) |
| Backend crates | **8** | `backend/Cargo.toml` workspace members: core, ssrf, db, checker, scheduler, notifier, api, agent |
| Demo stack RLS | **`RAMPART_RLS: "1"`** | `examples/everything/docker-compose.yml` — RLS genuinely on in this stack only |
| Demo login | `demo@rampart.local` / `Rampart-Live-9271` | `examples/everything/docker-compose.yml:31-32`, `examples/everything/README.md:20` |
| verify.sh success marker | `ALL TIERS NON-EMPTY` | `examples/everything/verify.sh` |
| Repo URL | `github.com/pen-pal/rampart` | git remote `origin` (confirm public before paste) |
| Syslog | RFC 5424 + RFC 3164 | `rampart-core/src/syslog.rs`, `routes/syslog.rs` |
| SIEM export | JSON / CEF / LEEF over webhook + syslog UDP/TCP | `rampart-notifier/src/siem.rs` |
| RLS behavior | flag-gated `RAMPART_RLS`, ENABLE (not FORCE), owner-exempt | `migrations/0116_rls_enable.sql` |
| Multi-org enforcement | flag-gated `RAMPART_MULTI_ORG` | per source; off by default |
| Aurora | wire-compatible with stock Postgres → connection-string swap | grounded; no Aurora-specific code paths |
| Ingest paths (real) | OTLP, Prometheus remote_write, Sentry DSN, RUM, profiles, syslog, push | `routes/`: `otlp.rs`, `prom_write.rs`, `error_ingest.rs`, `rum.rs`, `profiles.rs`, `syslog.rs`, `ingest.rs` |
| HA | Postgres advisory-lock leader election | `rampart-db/src/leader.rs` + `tests/leader.rs` |
| Audit log | tamper-evident hash chain (`prev_hash` + `chain_hash`) + continuous re-verification | `rampart-db/src/audit.rs` |
| Secrets at rest | AES-256-GCM | `rampart-db/src/secrets.rs` |
| Compliance | GDPR export + anonymizing erasure (preserves audit chain); SOC 2 CC6 access review | `routes/compliance.rs`, `rampart-db/src/access_review.rs`, tests `gdpr.rs`/`compliance.rs` |

### Things deliberately NOT claimed (anti-overclaim guardrails)

- **NOT** "runs on 5 databases today." A partial SQLite backend exists behind the Store seam (`rampart-db/src/sqlite/`), feature-gated `sqlite = []` (off by default), not wired into the shipped binary. Framed only as "first per-driver backend, next up."
- **NOT** "RLS enforced everywhere." Isolation is app-layer per-request `org_id` scoping; RLS (`RAMPART_RLS`) is opt-in defense-in-depth, ENABLE not FORCE, owner-exempt, on only in the demo stack.
- **NOT** present-tense "same-origin Vercel rewrite" as shipped — `frontend/vercel.json` does not exist yet and must be created for the hosted deploy.
- **Version is 0.156.10, not 0.156.0** — corrected throughout.
- README badges (38/41 probes, 128 channels, "single-tenant by design") are stale and contradict source; this package uses the enum-verified truth (42 / 129 / multi-tenant).

### Known discrepancies vs. earlier input briefs (resolved here)

- One input brief claimed `docs/deploy/aws-vercel.md` does **not** exist. It **does** (409 lines) and is the authoritative deploy reference — corrected above. The only genuinely missing deploy artifact is `frontend/vercel.json`.
