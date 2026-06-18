# Rampart — H0 Hackathon Submission Kit

Submission playbook for **H0: Hack the Zero Stack with Vercel v0 and AWS Databases**
(<https://h01.devpost.com/>). This is the one doc to work from when assembling the
Devpost entry. Everything here maps to a real, verifiable Rampart capability — no
vaporware.

---

## 1. TL;DR — what we're submitting

**Rampart** — a self-hosted **observability + SIEM platform** that unifies uptime
monitoring, distributed tracing, logs, metrics, RUM, profiling, error tracking,
on-call/escalations, status pages, and security-detection rules into a single
multi-tenant product. Built on a Rust workspace backend + React SPA, backed by
**Aurora PostgreSQL**, frontend shipped on **Vercel**.

- **Track:** **Monetizable B2B App** (primary) — observability/SIEM is a classic
  B2B SaaS sold to engineering & security teams in finance, tech, healthcare,
  insurance. Multi-tenancy (orgs + per-org RBAC + per-org ingest credentials,
  shipped Phases 1–5) is what makes it a real tenant-isolated B2B product, not a
  single-user tool. Secondary fit: **Open Innovation**.
- **AWS Database:** **Aurora PostgreSQL**. Rampart already runs on stock Postgres
  (sqlx, compile-checked queries, 110+ migrations) — Aurora PG is wire-compatible,
  so this is a connection-string swap, not a rewrite. This is our honest edge: a
  deep, deliberate relational data model (31 tenant tables, FK graph, retention
  pruning, composite per-org uniqueness) rather than a toy schema.
- **Frontend on Vercel:** the Vite/React SPA deploys as a static Vercel project;
  a v0-scaffolded Next.js landing/marketing shell fronts it (see §4 gap analysis
  for the exact deploy wiring + the one integration change needed).

---

## 2. The pitch (problem / who / why) — for the writeup + video narration

> Engineering and security teams pay for 4–6 separate tools — Datadog for metrics,
> Sentry for errors, PagerDuty for on-call, a status-page vendor, a SIEM for
> security detections, an uptime checker. Each is a separate bill, a separate
> data silo, a separate login. **Rampart collapses all of that into one
> self-hostable, multi-tenant platform.** One Postgres-backed datastore, one UI,
> one set of org-scoped credentials. You point your apps' OpenTelemetry exporters,
> Sentry DSN, Prometheus remote-write, and RUM snippet at Rampart and you get
> traces, logs, metrics, errors, real-user-monitoring, profiling, uptime probes,
> alerting/escalation, public status pages, and security detection rules — tenant-
> isolated, on infrastructure you control.

**Who it's for:** B2B — platform/SRE/security teams at companies who want one pane
of glass without sending telemetry to a third party, and MSPs/agencies who need
hard tenant isolation to run observability for many client orgs from one install.

**Why it's viable on this stack:** observability is *write-heavy, query-heavy,
retention-bound* — exactly what Aurora PostgreSQL is built to scale (storage
auto-grow, read replicas, fast failover) while keeping the relational integrity
Rampart's data model depends on. Vercel gives the operator/admin console a global
edge-delivered frontend with zero ops.

---

## 3. Architecture diagram (for the required diagram + the AWS-DB-connection proof)

```
                            ┌──────────────────────────────────────┐
   Browser (operators,      │            VERCEL (frontend)          │
   status-page viewers) ───▶│  v0/Next.js shell + Rampart React SPA │
                            │  (static assets, edge-delivered)      │
                            └───────────────────┬──────────────────┘
                                                │ HTTPS  (VITE_API_BASE → API origin)
                                                │ CORS allow-listed
                                                ▼
   Customer apps             ┌──────────────────────────────────────┐
   (any language) ──────────▶│        RAMPART API  (Rust, axum)      │
     • OTLP traces/logs      │   single binary on AWS (ECS Fargate / │
     • Prometheus remote-wr  │   EC2 / App Runner), leader-elected.   │
     • Sentry DSN (errors)   │   = API + ALL ingest + scheduler +     │
     • RUM snippet           │   notifier. Org resolved per request   │
     • push/heartbeat        │   (cookie session / api-key / ingest   │
                            │   key) → tenant-scoped reads & writes. │
                            └───────────────────┬──────────────────┘
                                                │ sqlx pool, TLS (sslmode=require)
                                                ▼
                            ┌──────────────────────────────────────┐
                            │        AURORA POSTGRESQL (AWS)        │
                            │  31 tenant tables, org_id-scoped, FK  │
                            │  graph, retention pruning, migrations │
                            │  run on boot. Storage auto-scaling.   │
                            └──────────────────────────────────────┘
```

> The diagram deliverable (Devpost requires one) should be exported as PNG/SVG from
> this shape. The **AWS-DB-usage screenshot** = the AWS console showing the Aurora
> PostgreSQL cluster + the Rampart `DATABASE_URL` pointing at the Aurora writer
> endpoint (redact credentials).

---

## 4. Qualification path + honest gap analysis

The hackathon mandates **(a)** one of Aurora PostgreSQL / Aurora DSQL / DynamoDB,
and **(b)** the frontend deployed on Vercel or v0.app. Where Rampart stands:

| Requirement | Status | Work to qualify |
|---|---|---|
| AWS Database = **Aurora PostgreSQL** | ✅ wire-compatible | Provision an Aurora PG cluster; set `DATABASE_URL=postgres://USER:PASS@CLUSTER-ENDPOINT:5432/rampart?sslmode=require`. Migrations run on boot (`sqlx::migrate!`). **No code change** — sqlx reads `sslmode` from the URL. |
| Aurora DSQL / DynamoDB | ❌ N/A | Not pursued — Rampart's relational model (FKs, joins, composite uniqueness, transactional alert routing) is the opposite of a KV/DSQL fit. Aurora PG is the deliberate choice; say so in the writeup (it reads as craftsmanship, not a gap). |
| **Frontend on Vercel** | ⚠️ needs deploy wiring | Vite SPA builds to static assets → deploy as a Vercel static project. **One integration change:** the SPA calls the API with **relative** paths (`frontend/src/lib/api.js:33`, assumes same-origin). For a Vercel-hosted frontend talking to an AWS-hosted API, either (1) add a `VITE_API_BASE` env the SPA prepends, **or** (2) use a Vercel `rewrites` rule proxying `/v1/*`, `/otlp/*`, `/rum/*`, etc. to the API origin. Option (2) avoids CORS and keeps cookies same-site — **recommended**. |
| Use **v0** to scaffold Next.js | ⚠️ partial | Generate a v0 Next.js marketing/landing + login shell that embeds/links the operator SPA. Satisfies "scaffold production-ready Next.js frontend with v0" while the dense operator console stays the proven React app. Document the v0 project in the writeup. |
| Backend hosting | ✅ container exists | Published GHCR image. Run on ECS Fargate / App Runner / EC2 in the same VPC as Aurora. Vercel = frontend only (Vercel can't host the long-lived Rust ingest/scheduler binary). |

**The single must-do code change** before deploy: make the API base configurable.
Recommended Vercel `vercel.json` rewrite (no code change, no CORS):

```json
{
  "rewrites": [
    { "source": "/v1/:path*",     "destination": "https://api.YOURDOMAIN/v1/:path*" },
    { "source": "/otlp/:path*",   "destination": "https://api.YOURDOMAIN/otlp/:path*" },
    { "source": "/rum/:path*",    "destination": "https://api.YOURDOMAIN/rum/:path*" },
    { "source": "/push/:path*",   "destination": "https://api.YOURDOMAIN/push/:path*" },
    { "source": "/profiles/:path*","destination": "https://api.YOURDOMAIN/profiles/:path*" }
  ]
}
```

---

## 5. Devpost submission fields — drafts

**Which AWS Database did you use?**
> Aurora PostgreSQL. Rampart's entire data model (31 tenant-scoped tables, foreign-
> key graph, composite per-org uniqueness, transactional alert routing, time-series
> retention pruning) is relational by design. We run on Aurora PG via a standard
> connection string with `sslmode=require`; migrations apply automatically on boot.

**Inspiration / what it does / how we built it / challenges / accomplishments / what's next**
- *Inspiration:* teams drowning in 5+ observability/security SaaS bills and data silos.
- *What it does:* one multi-tenant platform — uptime, traces, logs, metrics, RUM,
  profiling, errors, on-call/escalations, status pages, security detection rules.
- *How we built it:* Rust (axum) single-binary backend = API + all ingest +
  scheduler + notifier, leader-elected for HA; React SPA; **Aurora PostgreSQL**;
  frontend on **Vercel**. OpenTelemetry-native ingest (OTLP), Sentry-compatible
  error DSN, Prometheus remote-write, browser RUM snippet.
- *Challenges:* tenant isolation across every read/write path without breaking the
  single-org install (solved with org-scoped queries + per-org credentials + a
  reversible enforcement-flip plan); secrets-at-rest; SSRF-guarded outbound.
- *What's next:* Aurora read-replica routing for the query tier; RLS as defense-in-depth.

**Vercel Project Link + Team ID:** _(fill after deploy)_
**Architecture diagram:** §3 above, exported.
**AWS DB proof screenshot:** Aurora console + `DATABASE_URL` config (redacted).

---

## 6. Demo video script (< 3 minutes, YouTube)

1. **0:00–0:25 — Problem.** "Teams pay for Datadog + Sentry + PagerDuty + a status
   page + a SIEM. Five bills, five silos. Rampart is all of it, multi-tenant, on
   your own infra, on Aurora PostgreSQL."
2. **0:25–0:45 — Stack.** Show the architecture diagram. Name it: Vercel frontend →
   Rust API on AWS → Aurora PostgreSQL. Show the Aurora console (the DB-proof shot).
3. **0:45–2:30 — Working app (the `examples/everything` demo drives REAL data).**
   Boot the everything-demo; show live in the UI: a trace waterfall from the demo
   backend, structured logs, a metric chart, a RUM session, an error grouped in a
   project, an uptime monitor flipping + firing an alert + an escalation, a public
   status page, a security detection rule matching. Switch orgs to show tenant
   isolation. This is the money shot — real telemetry, not seeds.
4. **2:30–2:55 — Impact + DB.** "Observability is write- and query-heavy with
   retention — Aurora PostgreSQL scales the storage and read path while keeping the
   relational integrity our routing and isolation depend on."
5. **2:55–3:00 — Close.** Repo + live Vercel link.

> The `examples/everything` stack (see `docs/DEMO.md` + `examples/everything/`) is
> purpose-built to emit genuine traces/logs/metrics/RUM/errors from a real demo
> frontend+backend — use it for the footage so judges see live data.

---

## 7. Submission checklist

- [ ] Aurora PostgreSQL cluster provisioned (request AWS credits via the H0 form).
- [ ] Rampart API deployed on AWS (ECS Fargate / App Runner) with `DATABASE_URL`
      → Aurora writer endpoint, `sslmode=require`; confirm migrations ran on boot.
- [ ] API base wired (`vercel.json` rewrites — §4) so the SPA reaches the API.
- [ ] v0 Next.js shell generated + frontend deployed on Vercel; capture **Project
      Link + Team ID**.
- [ ] Architecture diagram exported (PNG/SVG) from §3.
- [ ] AWS-DB-usage screenshot captured (Aurora console + redacted config).
- [ ] < 3-min demo video recorded (script §6) + uploaded to YouTube.
- [ ] Devpost text fields filled (§5), track = **Monetizable B2B App**.
- [ ] (Bonus) Publish a build blog/video, hashtag **#H0Hackathon**, with the
      required "created for this hackathon" statement.

## 8. Timeline

- **Today:** 2026-06-18.
- **Hackathon period start:** June 29, 2026 @ 8:00pm EDT.
- **Submission deadline:** **June 29, 2026 @ 5:00pm PDT** — tight; the deploy +
  video are the long poles. Aurora swap + Vercel rewrite are hours, not days.

## 9. Judging-criteria alignment (build the narrative around these)

- **Technological Implementation:** Rust single-binary ingest+scheduler+notifier,
  OTel-native, leader-elected HA, secrets-at-rest, SSRF-guarded; a deliberate
  31-table relational model on Aurora PG (not a toy schema). This is our strongest
  axis — lean on it.
- **Design:** dense-but-coherent operator console; one UI for tiers that are
  normally 5 separate products; org switcher for tenant context.
- **Impact:** collapses 5–6 SaaS bills + data silos into one self-hosted, tenant-
  isolated platform — concrete cost + data-sovereignty story.
- **Originality:** "self-hosted multi-tenant observability *and* SIEM in one Rust
  binary on Aurora" is a genuinely uncommon combination.
