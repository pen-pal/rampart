# Rampart — H0 Hackathon Submission Kit

The **single source of truth** for the H0 Devpost entry
(<https://h01.devpost.com/>, deadline **2026-06-29 17:00 PDT**). Everything here
maps to a real, verifiable Rampart capability — no vaporware.

- The click-by-click **deploy mechanics** (Aurora, AWS hosting, Vercel, v0) live
  in [`deploy/aws-vercel.md`](deploy/aws-vercel.md). This doc is the **plan + the
  copy + the checklist**; that doc is the **how-to**.
- Owner-action gates (need a human / account / recording) are flagged
  **🔴 OWNER** throughout.

---

## 0. Submission runbook (now → 2026-06-29 17:00 PDT)

Day-by-day, each step with the action + what it needs. The deadline is firm; the
**deploy + the video** are the long poles, so they're front-loaded. (The
hackathon *submission window* opens before the deadline — submit the moment the
assets are ready; you can keep editing the Devpost entry until close.)

### Phase 1 — Accounts + credits (do immediately) 🔴 OWNER
1. **Create/confirm accounts:** AWS, Vercel, v0. → `deploy/aws-vercel.md §0`.
2. **Request AWS credits** via the H0 form (Aurora bills hourly; don't wait).
3. **Note your Vercel Team ID** now (Settings → General) so it's ready for the
   form. → `deploy/aws-vercel.md §3`.

### Phase 2 — Backend on AWS + Aurora (½–1 day)
4. **Provision Aurora PostgreSQL** (Serverless v2, private subnets, SG open to
   the backend SG on 5432, initial DB `rampart`). → `aws-vercel.md §1`.
5. **Build the `DATABASE_URL`** with `?sslmode=require`. → `§1`.
6. **Deploy the backend** — pick App Runner (least ops) / ECS Fargate (most
   AWS-native) / EC2. Mirror `ghcr.io/pen-pal/rampart` → ECR if using
   App Runner/ECS. Env: `DATABASE_URL`, `RAMPART_SECRET_KEY=$(openssl rand -hex
   32)`, `BIND_ADDR=0.0.0.0:3000`. → `§2`.
7. **Verify migrations ran on boot:** `curl https://api.YOURDOMAIN/readyz` → 200,
   and `psql "$DATABASE_URL" -c "select count(*) from _sqlx_migrations;"`. → `§1`.
8. **Create the first admin** (signup form on first visit, or
   `reset-password`). → `§2`.

### Phase 3 — Frontend on Vercel + v0 (½ day)
9. **Author `frontend/vercel.json`** with the same-origin `/v1/*` rewrite → the
   AWS API origin. (No product code change — this is the only deploy artifact you
   write.) → `§3`.
10. **Deploy the SPA to Vercel** (CLI `vercel --prod`, root dir `frontend`,
    Vite preset). Smoke-test: login works, `/v1/...` returns 200 in DevTools. →
    `§3`.
11. **Scaffold the v0 Next.js landing/login shell** → one-click Deploy to
    Vercel; CTA links to the console URL. → `§4`.
12. **Capture the Project Link + Team ID.** → `§3`.

### Phase 4 — Assets (1 day) 🔴 OWNER (recording + screenshots)
13. **Capture the AWS-DB-usage screenshot(s):** Aurora console (engine =
    *Aurora PostgreSQL*, status Available, Monitoring graphs) + redacted
    `DATABASE_URL` + (bonus) `_sqlx_migrations` count. → `aws-vercel.md §1`.
14. **Export the architecture diagram** (§4 below) to PNG/SVG.
15. **Record the <3-min demo video** (shot list §6) using
    `examples/everything` for live data; **upload to YouTube** (unlisted or
    public). → §6 + `aws-vercel.md §5`.

### Phase 5 — Devpost entry + submit (½ day) 🔴 OWNER
16. **Create the Devpost project**, track = **Monetizable B2B App**.
17. **Paste the final writeup** (§5) + the "Which AWS DB" answer (§5).
18. **Attach** YouTube link, Vercel Project Link, Team ID, architecture diagram,
    AWS-DB screenshot, repo link.
19. **Run the pre-submit checklist** (§7) — every field has an owner box.
20. **Submit.** (Optional bonus: a build blog/video with `#H0Hackathon` + the
    "created for this hackathon" statement.)

### Buffer
21. Leave **≥1 day** before 2026-06-29 17:00 PDT for re-recording / a broken
    rewrite / an Aurora SG mistake. Don't aim for the last hour.

---

## 1. TL;DR — what we're submitting

**Rampart** — a self-hosted **observability + SIEM platform** that unifies uptime
monitoring, distributed tracing, logs, metrics, RUM, profiling, error tracking,
on-call/escalations, status pages, and security-detection rules into a single
multi-tenant product. Rust workspace backend + React SPA, backed by **Aurora
PostgreSQL**, frontend shipped on **Vercel**.

- **Track:** **Monetizable B2B App** (primary). Observability/SIEM is a classic
  B2B SaaS sold to engineering & security teams in finance, tech, healthcare,
  insurance. Multi-tenancy (orgs + per-org RBAC + per-org ingest credentials,
  shipped Phases 1–5, with Postgres RLS as defense-in-depth) is what makes it a
  real tenant-isolated B2B product, not a single-user tool. Secondary fit:
  **Open Innovation**.
- **AWS Database:** **Aurora PostgreSQL**. Rampart already runs on stock Postgres
  (sqlx, compile-checked queries, 150+ migrations) — Aurora PG is wire-compatible,
  so this is a connection-string swap, not a rewrite. Our honest edge: a deep,
  deliberate relational data model (org-scoped tenant tables, FK graph, retention
  pruning, composite per-org uniqueness) rather than a toy schema.
- **Frontend on Vercel:** the Vite/React SPA deploys as a static Vercel project
  with a `vercel.json` same-origin rewrite to the AWS API (no CORS, no code
  change); a **v0-scaffolded Next.js** landing/login shell fronts it.

---

## 2. The pitch (for the writeup + video narration)

> Engineering and security teams pay for 4–6 separate tools — Datadog for
> metrics, Sentry for errors, PagerDuty for on-call, a status-page vendor, a SIEM
> for security detections, an uptime checker. Each is a separate bill, a separate
> data silo, a separate login. **Rampart collapses all of that into one
> self-hostable, multi-tenant platform.** One Postgres-backed datastore, one UI,
> one set of org-scoped credentials. Point your apps' OpenTelemetry exporters,
> Sentry DSN, Prometheus remote-write, and RUM snippet at Rampart and you get
> traces, logs, metrics, errors, real-user-monitoring, profiling, uptime probes,
> alerting/escalation, public status pages, and security detection rules —
> tenant-isolated, on infrastructure you control.

**Who it's for:** B2B — platform/SRE/security teams who want one pane of glass
without sending telemetry to a third party, and MSPs/agencies who need hard
tenant isolation to run observability for many client orgs from one install.

**Why it's viable on this stack:** observability is *write-heavy, query-heavy,
retention-bound* — exactly what Aurora PostgreSQL is built to scale (storage
auto-grow, Serverless v2, read replicas, fast failover) while keeping the
relational integrity Rampart's routing + isolation depend on. Vercel gives the
operator console a global edge-delivered frontend with zero ops.

---

## 3. Demo video script + shot list (< 3 min, YouTube) 🔴 OWNER

Refined from the live `examples/everything` stack (real traces/logs/metrics/RUM/
errors/monitors/incidents/status-page/RLS multi-tenancy). Each shot is tied to a
judging criterion. Run `bash examples/everything/verify.sh` first to confirm
every tier is non-empty, and give the stack ~3–5 min of uptime so monitors flap
and SLO budgets burn.

| Time | Shot | On screen | Narration beat | Judging criterion |
| --- | --- | --- | --- | --- |
| 0:00–0:20 | **Problem** | Title card / 5 vendor logos crossed out | "Teams pay for Datadog + Sentry + PagerDuty + a status page + a SIEM. Five bills, five silos. Rampart is all of it — multi-tenant, on your own infra, on Aurora PostgreSQL." | Impact |
| 0:20–0:35 | **Stack + DB proof** | Architecture diagram → cut to **Aurora console** (engine, Available, Monitoring graph) | "Vercel frontend → a Rust API on AWS → Aurora PostgreSQL. Here's the live cluster." | Tech Impl |
| 0:35–0:55 | **Traces + logs + correlation** | Trace waterfall (`/api/checkout`, errored leaf span) → click into the linked log line carrying the trace id | "Real OpenTelemetry traces, structured logs, correlated by trace id — not seed data." | Tech Impl |
| 0:55–1:10 | **Metrics + RUM + profiling** | A metric chart (`demo_queue_depth`) → a RUM web-vitals session (poor LCP) → a flame/folded profile | "Prometheus remote-write metrics, real-user web vitals from a browser, continuous CPU profiles." | Tech Impl / Design |
| 1:10–1:30 | **Errors + uptime + alerting** | Error issue grouped in a project (users-affected, by-release) → a monitor flipping **Down** → an escalation/on-call page firing | "Sentry-compatible error tracking, an uptime monitor genuinely flipping, escalation paging on-call." | Impact / Design |
| 1:30–1:45 | **Status page + SIEM** | Public status page with an open incident → a detection rule matching `failed login` and raising a finding | "A public status page, and a SIEM detection rule firing on real auth-failure logs." | Originality |
| 1:45–2:15 | **Multi-tenancy (the money shot)** | Org switcher: switch from `Default` to `Demo Team` → its *own* monitors + telemetry → attempt to view the other org's resource → 404 | "Two orgs, each with their own data and credentials. Switch tenants — you only ever see your org. Isolation is enforced at the Postgres layer with row-level security, on top of app-level scoping." | Originality / Tech Impl |
| 2:15–2:40 | **One UI, one binary** | Quick pan across the nav: uptime, traces, logs, metrics, RUM, profiling, errors, on-call, status, detections | "Every tier that's normally five products — in one Rust binary, one Postgres database, one UI." | Design / Impact |
| 2:40–2:55 | **DB rationale** | Back to Aurora Monitoring graph | "Observability is write- and query-heavy with retention — Aurora PostgreSQL scales the storage and read path while keeping the relational integrity our routing and isolation depend on." | Tech Impl |
| 2:55–3:00 | **Close** | Repo URL + live Vercel link | "Self-hosted observability and SIEM, on Aurora. Repo and live demo in the description." | — |

**Recording notes:** see `aws-vercel.md §5` for record-locally vs.
record-against-the-deployed-stack. Record locally against
`examples/everything` for the feature tour (richest data), and show the **live
Aurora console** for the DB proof — that split is honest and standard. If you
have time, point the everything-stack exporters at the AWS origin so every tier
genuinely flows into Aurora.

---

## 4. Architecture diagram (export to PNG/SVG for the upload)

```
                            ┌──────────────────────────────────────┐
   Browser (operators,      │            VERCEL (frontend)          │
   status-page viewers) ───▶│  v0/Next.js shell + Rampart React SPA │
                            │  (static assets, edge-delivered;       │
                            │   vercel.json same-origin /v1 rewrite) │
                            └───────────────────┬──────────────────┘
                                                │ HTTPS, same-origin proxy
                                                │ (cookie session flows; no CORS)
                                                ▼
   Customer apps             ┌──────────────────────────────────────┐
   (any language) ──────────▶│        RAMPART API  (Rust, axum)      │
     • OTLP traces/logs      │   single binary on AWS (App Runner /   │
     • Prometheus remote-wr  │   ECS Fargate / EC2), leader-elected.  │
     • Sentry DSN (errors)   │   = API + ALL ingest + scheduler +     │
     • RUM snippet           │   notifier. Org resolved per request   │
     • push/heartbeat        │   (cookie session / api-key / ingest   │
   (point exporters at the   │   key) → tenant-scoped reads & writes. │
    AWS origin, not Vercel)  └───────────────────┬──────────────────┘
                                                │ sqlx pool, TLS (sslmode=require)
                                                ▼
                            ┌──────────────────────────────────────┐
                            │        AURORA POSTGRESQL (AWS)        │
                            │  org_id-scoped tenant tables, FK      │
                            │  graph, retention pruning, migrations │
                            │  run on boot. Serverless v2 storage   │
                            │  auto-scaling. Optional RLS isolation. │
                            └──────────────────────────────────────┘
```

> **Export:** paste the ASCII into a diagram tool (e.g. <https://asciiflow.com>,
> Excalidraw, or draw.io) and re-draw as boxes, or screenshot a monospace render
> at 2× scale. Devpost wants an image (PNG/SVG). The **AWS-DB-usage screenshot**
> is separate (Aurora console + redacted `DATABASE_URL`) — see `aws-vercel.md §1`.

---

## 5. Devpost writeup — FINAL copy (paste-ready)

### Which AWS Database did you use?
> **Aurora PostgreSQL.** Rampart's entire data model is relational by design —
> org-scoped tenant tables with a foreign-key graph, composite per-org
> uniqueness constraints, transactional alert routing, and time-series retention
> pruning. We run on Aurora PostgreSQL via a standard connection string with
> `sslmode=require`; all 150+ migrations apply automatically on boot. Because
> Aurora PostgreSQL is wire-compatible with stock Postgres and we use sqlx (which
> reads TLS settings straight from the connection URL), moving to Aurora was a
> connection-string change, not a rewrite — the depth is in the schema, which
> Aurora's storage auto-scaling, Serverless v2, and read-path scaling are built
> for.

### Inspiration
> Every engineering team we know pays for a stack of overlapping SaaS: Datadog
> for metrics, Sentry for errors, PagerDuty for on-call, a status-page vendor, a
> SIEM for security detections, an uptime checker. Five-plus bills, five-plus
> data silos, five-plus logins — and your telemetry, often your most sensitive
> data, lives on someone else's servers. We wanted one platform that does all of
> it, that you can run on your own infrastructure, and that's genuinely
> multi-tenant so an agency or platform team can isolate many customers from one
> install.

### What it does
> Rampart is a self-hosted observability **and** SIEM platform. From one UI and
> one Postgres-backed datastore it gives you: uptime monitoring (40+ probe
> kinds), distributed tracing (OpenTelemetry-native), structured logs, metrics
> (Prometheus remote-write + a push API), real-user monitoring, continuous
> profiling, Sentry-compatible error tracking, on-call schedules + escalation
> policies, public status pages with incidents and subscribers, and security
> detection rules that raise findings from your logs. Everything is org-scoped:
> per-org RBAC, per-org ingest credentials, an org switcher, and Postgres
> row-level security as defense-in-depth, so tenants never see each other's data.

### How we built it
> A Rust (axum) single-binary backend is the whole server: the API, **every**
> ingest listener (OTLP, Prometheus remote-write, Sentry DSN, RUM beacons,
> profiles, push/heartbeats), the scheduler that runs the probes, and the
> notifier that fans alerts out to 120+ channel kinds — all leader-elected for
> HA. Data lives in **Aurora PostgreSQL** via sqlx with compile-time-checked
> queries and 150+ forward-only migrations that run on boot. The operator console
> is a React/Vite SPA deployed on **Vercel** as a static project, with a
> `vercel.json` same-origin rewrite proxying `/v1` to the AWS-hosted API (so the
> session cookie flows with zero CORS), and a **v0-scaffolded Next.js** landing/
> login shell in front. The backend container (`ghcr.io/pen-pal/rampart`) runs on
> AWS in the same VPC as Aurora.

### Challenges we ran into
> The hard part was **tenant isolation across every read and write path** without
> breaking the single-org install. We solved it by threading an `OrgId` through
> every repository function, adding `*_all`/`*_unscoped` siblings for system
> loops, per-org ingest credentials, and a *reversible* Postgres row-level
> security layer (off by default, owner-bypass, no schema lock-in) as
> defense-in-depth. Other challenges: secrets-at-rest (AES-256-GCM for channel
> credentials, fail-closed when a key is set), SSRF-guarded outbound for
> user-defined probe targets and webhooks, and making the Vercel frontend talk to
> an AWS API origin without a code change — the same-origin rewrite was the clean
> answer.

### Accomplishments we're proud of
> One Rust binary that ingests OpenTelemetry, Prometheus, Sentry, and RUM
> *simultaneously*, probes 40+ monitor kinds, pages on-call, serves public status
> pages, and runs SIEM detections — all tenant-isolated on a deliberately deep
> relational schema. The `examples/everything` stack proves it end to end:
> `docker compose up` brings up a live system where every tier is filled with
> **real** data (a real instrumented app's traces/logs/metrics/profiles/errors, a
> real browser's RUM, Prometheus remote-write, Alertmanager-driven incidents,
> genuinely flapping monitors, and two isolated orgs) — no fabricated rows.

### What's next
> Aurora read-replica routing for the query tier (point reads at a reader
> endpoint while writes hit the writer); promoting RLS from defense-in-depth to
> the enforced default; per-tenant data-retention tiers; and a hosted multi-org
> SaaS offering built on the same binary.

### Submission links (fill after deploy) 🔴 OWNER
- **Vercel Project Link:** ___________ (`aws-vercel.md §3`)
- **Vercel Team ID:** ___________
- **Demo video (YouTube):** ___________
- **Architecture diagram:** §4, exported to PNG/SVG
- **AWS-DB-usage screenshot:** Aurora console + redacted `DATABASE_URL`
- **Repo:** <https://github.com/pen-pal/rampart>

---

## 6. Qualification path + readiness GAPS (honest)

The rules mandate **(a)** Aurora PostgreSQL / Aurora DSQL / DynamoDB and **(b)**
the frontend on Vercel or v0.app. Status:

| Requirement | Status | Closing action |
| --- | --- | --- |
| AWS DB = **Aurora PostgreSQL** | ✅ wire-compatible, **no code change** (verified: `connect()` passes the URL to sqlx, which parses `sslmode`; `migrate()` runs on boot) | Provision + set `DATABASE_URL=…?sslmode=require`. `aws-vercel.md §1`. |
| Aurora DSQL / DynamoDB | ❌ N/A by design | Not pursued — the relational model (FKs, joins, composite uniqueness, transactional routing) is the opposite of a KV fit. Say so in the writeup; it reads as craftsmanship. |
| **Frontend on Vercel** | ⚠️ needs the rewrite | Author `frontend/vercel.json` (same-origin `/v1` proxy). `aws-vercel.md §3`. |
| Use **v0** to scaffold Next.js | ⚠️ to-do | Generate a v0 Next.js landing/login shell; one-click deploy. `aws-vercel.md §4`. |
| Backend hosting | ✅ image exists | Run `ghcr.io/pen-pal/rampart` on App Runner / ECS / EC2 in Aurora's VPC. `aws-vercel.md §2`. |

### The readiness gaps that would weaken the submission — and how to close them

1. **The SPA assumes a same-origin API (relative paths, no `VITE_API_BASE`).**
   *Verified:* `frontend/src/lib/api.js` fetches `/v1/...` with
   `credentials: 'same-origin'`, and the API's CORS is `allow_origin(Any)`
   **without** `allow_credentials` (intentional invariant,
   `rampart-api/src/lib.rs:52`). So a cross-origin Vercel→AWS call **cannot**
   carry the session cookie. **Close it** with the `vercel.json` same-origin
   rewrite (no code change) — `aws-vercel.md §3`. *Do not* try plain CORS; it
   silently breaks auth. This is the single most important gotcha.

2. **`RAMPART_SECRET_KEY` ↔ live-delivery interaction in the demo.** The
   `examples/everything` README documents an upstream bug where, *with a key
   set*, the monitor-flip notification path fails `missing field url` (it reads
   channel config without decrypting), while `/test`/digest/scheduled paths work.
   **Close it for the video** by choosing one: (a) set the key (correct, secure)
   and demo `/test`-fired + digest deliveries, or (b) leave it unset to show live
   flip-path deliveries. For the **production AWS deploy**, set the key — secrets-
   at-rest is part of the security story. Don't claim both at once on camera.

3. **First-run onboarding is a bare signup form.** First visit shows signup only
   when zero users exist; first user becomes admin. For the demo, **pre-seed an
   admin** (`reset-password`) or pre-create it so you're not filming a blank
   signup. Consider `seed-demo` against Aurora for a populated dashboard if you
   record from the deployed instance. `aws-vercel.md §2`.

4. **Ingest paths must NOT be proxied through Vercel.** Customer exporters
   (OTLP/Prometheus/Sentry/RUM/profiles) should hit the **AWS API origin
   directly**, not the Vercel domain (Vercel function limits + the SPA never
   calls them). The earlier `vercel.json` draft over-listed these; the corrected
   rewrite in `aws-vercel.md §3` proxies only `/v1` (+ `/push`, `/healthz`,
   `/readyz`). Point the everything-stack exporters at the AWS origin if
   recording against the deploy.

5. **Demo polish / timing.** The everything-stack needs **3–5 min of uptime**
   before monitors flap and SLO budgets burn; `verify.sh` must pass before you
   hit record. Budget a re-take — the multi-tenancy org-switch shot is the
   originality money shot and must be clean.

6. **Aurora networking is the easiest thing to get wrong.** The DB SG must allow
   inbound 5432 **from the backend SG** (not 0.0.0.0/0), and App Runner needs a
   **VPC connector** to reach a private cluster. Test `/readyz` → 200 before
   wiring Vercel. `aws-vercel.md §1–§2`.

---

## 7. Pre-submit checklist (every field + asset, with an owner box)

Mark `[x]` and note the owner. 🔴 = needs a human action (account/recording/UI).

**Infra**
- [ ] Aurora PostgreSQL cluster provisioned, status Available 🔴
- [ ] AWS credits requested via H0 form 🔴
- [ ] Backend deployed (App Runner / ECS / EC2) with `DATABASE_URL`,
      `RAMPART_SECRET_KEY`, `BIND_ADDR`
- [ ] `/readyz` returns 200 (DB-gated) — migrations confirmed run on boot
- [ ] First admin created
- [ ] `frontend/vercel.json` same-origin `/v1` rewrite → API origin
- [ ] SPA deployed to Vercel; login + `/v1` requests 200 in DevTools
- [ ] v0 Next.js shell scaffolded + deployed 🔴

**Assets**
- [ ] Architecture diagram exported to PNG/SVG (§4)
- [ ] AWS-DB-usage screenshot: Aurora console + redacted `DATABASE_URL`
      (+ bonus `_sqlx_migrations` count) 🔴
- [ ] <3-min demo video recorded (shot list §3) 🔴
- [ ] Video uploaded to YouTube (unlisted/public); link copied 🔴

**Devpost fields**
- [ ] Project created, track = **Monetizable B2B App**
- [ ] "Which AWS Database" answer pasted (§5)
- [ ] Inspiration / What-it-does / How-built / Challenges / Accomplishments /
      What's-next pasted (§5)
- [ ] Vercel **Project Link** filled 🔴
- [ ] Vercel **Team ID** filled 🔴
- [ ] YouTube demo link filled 🔴
- [ ] Architecture diagram attached
- [ ] AWS-DB screenshot attached
- [ ] Repo link attached
- [ ] **SUBMITTED** before 2026-06-29 17:00 PDT 🔴

**Bonus**
- [ ] Build blog/video published with `#H0Hackathon` + the "created for this
      hackathon" statement

---

## 8. Judging-criteria alignment (build the narrative around these)

- **Technological Implementation:** Rust single-binary ingest + scheduler +
  notifier, OTel-native, leader-elected HA, secrets-at-rest, SSRF-guarded; a
  deliberate org-scoped relational model on Aurora PG with RLS isolation (not a
  toy schema). **Strongest axis — lean on it.**
- **Design:** dense-but-coherent operator console; one UI for tiers that are
  normally five separate products; org switcher for tenant context.
- **Impact:** collapses 5–6 SaaS bills + data silos into one self-hosted,
  tenant-isolated platform — a concrete cost + data-sovereignty story.
- **Originality:** self-hosted multi-tenant observability **and** SIEM in one Rust
  binary on Aurora, with Postgres-RLS tenant isolation — a genuinely uncommon
  combination.

---

## 9. Reference index

- Deploy mechanics (Aurora / AWS / Vercel / v0): [`deploy/aws-vercel.md`](deploy/aws-vercel.md)
- Self-host setup paths: [`SETUP.md`](SETUP.md)
- The live demo stack: [`DEMO.md`](DEMO.md) + [`../examples/everything/README.md`](../examples/everything/README.md)
- Multi-tenancy design: [`MULTITENANCY.md`](MULTITENANCY.md)
- Single-box deploy artifacts (systemd, backups, reverse proxy): [`deploy/README.md`](deploy/README.md)
