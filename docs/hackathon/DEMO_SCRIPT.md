# Rampart — Demo Video Script (< 3:00)

> The shot list + narration for the H0 Devpost demo video. Hard cap **3:00**;
> target **2:50**. Every beat is runnable against the `examples/everything` stack
> on `http://localhost:3000` plus one browser tab on the live Aurora console.
>
> Companions: paste-ready copy → [`SUBMISSION.md`](SUBMISSION.md); zero-to-live
> deploy → [`GO_LIVE.md`](GO_LIVE.md); field readiness → [`CHECKLIST.md`](CHECKLIST.md).
> Everything below maps to shipped code at workspace **v0.157.12** and a tier the
> `examples/everything` stack fills with **real** data — no fabricated rows.

---

## Before you hit record (≈ 8 min of prep)

```bash
cd examples/everything
cp .env.example .env          # defaults work out of the box
docker compose up             # default profile is enough; --profile heavy is optional
```

1. **Leave `RAMPART_SECRET_KEY` unset in `.env`** (the default). See the honesty
   rule below — with a key set, live monitor-flip deliveries degrade on the
   published image, so unset keeps the headline "real deliveries" working.
2. **Let it run ≥ 5 minutes** so monitors flap, episodes open, SLO budgets burn,
   incidents open/close, and detection findings accrue. The remote-agent service
   compiles from source on first `up` — give it a few minutes.
3. **Gate on green:**
   ```bash
   bash verify.sh        # must print: ✅ ALL TIERS NON-EMPTY
   ```
   Do not record until this is green.
4. **Log in** at <http://localhost:3000> as `demo@rampart.local` /
   `Rampart-Live-9271`. Pre-open (pre-warm) every tab you'll click so nothing is
   mid-load on camera.
5. **Second browser tab on the live Aurora console** — engine *Aurora
   PostgreSQL*, status *Available*, the **Monitoring** tab with a live CPU/
   connections graph. This is the AWS-DB proof shot. (Recording the feature tour
   locally against the rich `everything` data and cutting to the live Aurora
   console for the DB proof is the standard, honest split — see
   [`../deploy/aws-vercel.md` §5](../deploy/aws-vercel.md).)
6. Record 1080p+, mouse slow and deliberate, no dead air. Narration below is
   ≈ 360 words, paced for ~2:50.

### The one on-camera honesty rule

The published image has an upstream quirk: with `RAMPART_SECRET_KEY` **set**, the
live monitor-flip notification path fails to decrypt channel config. The demo
ships with the key **unset** so flip-path deliveries are real. On camera, either
show **live deliveries** (key unset — the default) **or** talk about
**encryption-at-rest** — never both in the same breath.

Other guardrails to honour in the narration:
- **Tenancy:** "isolation is per-request `org_id` scoping in the app, with
  Postgres row-level security enabled here as defense-in-depth." Do **not** say
  "RLS enforced everywhere."
- **Multi-backend:** if you mention it, "Postgres and SQLite run the full
  monitoring stack; MySQL serves the management API today." Do **not** imply
  MySQL drives the scheduler/alerting.
- **Migrations:** the safe spoken line is "100-plus migrations" unless the exact
  number (118) is on screen.

---

## Scene-by-scene (target 2:50)

| # | Time | Show on screen | Say (narration) |
|---|------|----------------|-----------------|
| 1 | 0:00–0:18 | **Title card** "Rampart — one binary, every signal", then 5 vendor logos (Datadog, Sentry, PagerDuty, a status-page vendor, a SIEM) crossed out → the Rampart logo. | "Engineering and security teams pay for five separate tools — metrics, errors, on-call, a status page, a SIEM. Five bills, five data silos, five logins. Rampart is all of it: one self-hosted, multi-tenant platform, on Aurora PostgreSQL." |
| 2 | 0:18–0:33 | The **architecture diagram** (Mermaid block in `SUBMISSION.md`, exported to PNG), then **cut to the live Aurora console**: engine *Aurora PostgreSQL*, status Available, the Monitoring graph. | "A Vercel frontend talks to a single Rust binary on AWS, backed by Aurora PostgreSQL. Here's the live cluster — over a hundred migrations ran on boot, on a relational schema built for this workload." |
| 3 | 0:33–0:53 | **Traces** → open the `/api/checkout` trace → the waterfall with the errored leaf span → click through to the correlated **log line** carrying the same `trace_id`. | "Real OpenTelemetry traces from an instrumented app — an Express service through Postgres and Redis. Drill into the errored span and jump straight to the correlated log line by trace id. This is live OTLP, not seed data." |
| 4 | 0:53–1:10 | **Metrics** → the `demo_queue_depth` chart breaching its rule → **RUM** → a web-vitals session with a poor LCP → **Profiling** → an interactive flamegraph. | "Prometheus remote-write metrics with rule-based alerting, real-user web vitals from an actual browser page, and continuous CPU profiles rendered as a flamegraph — all ingested live." |
| 5 | 1:10–1:30 | **Errors** → an issue grouped in a project (users-affected, by-release) → **Monitors** → `edge · flapping ready probe` showing a Down flip + uptime strip → the **on-call** episode that paged. | "Sentry-compatible error tracking, grouped by release with users-affected counts — point your existing DSN at Rampart, no SDK swap. An uptime monitor genuinely flipping Down, and the escalation policy paging the on-call schedule for real." |
| 6 | 1:30–1:48 | **Status pages** → the public page with an open incident + updates → **Detections** → the `failed login` SIEM rule with raised **findings**. | "A public status page driven by real incidents. And the security side: a SIEM detection rule firing on real auth-failure logs and raising findings — observability and security in one product." |
| 7 | 1:48–2:18 | **THE MONEY SHOT.** Org switcher: `Default` → `Demo Team` → its own `demo-team ·` monitors + telemetry. Then paste a `Default`-only resource URL → **404 / not visible**. | "Two tenants, each with their own monitors, telemetry, and ingest credentials. Switch orgs — you only ever see your own. Try to reach the other org's resource by URL and it's gone. Isolation is per-request org scoping in the app, with Postgres row-level security on here as defense-in-depth." |
| 8 | 2:18–2:38 | Slow pan across the left nav: Uptime, Traces, Logs, Metrics, RUM, Profiling, Errors, On-call, Status, Detections. | "Every tier that's normally five separate products — uptime, tracing, logs, metrics, RUM, profiling, errors, on-call, status pages, detections — in one Rust binary, one Postgres database, one UI." |
| 9 | 2:38–2:52 | Back to the **Aurora Monitoring graph**; optionally overlay one line of the hardening story (e.g. the SSE org-filter fix or the `/unlock` rate limit). | "It's hardened like a product, not a demo — we closed cross-tenant telemetry leaks, rate-limited the public DoS surface, and made bearer auth crash-safe, each with a regression test. Observability is write-heavy and retention-bound — exactly what Aurora PostgreSQL scales while keeping the relational integrity our tenant isolation depends on." |
| 10 | 2:52–3:00 | **Repo URL + live Vercel link** on screen. | "Self-hosted observability and SIEM, multi-tenant, on Aurora. Repo and live demo in the description." |

---

## Capture / quality notes

- **Scene 7 is the originality money shot** — budget a re-take so the org switch
  and the 404 are clean and obvious. Have the `Default`-only URL on the clipboard
  before you start so the paste is instant.
- **Fallback if a tier looks thin on camera:** give the stack more uptime and
  re-run `verify.sh`; traces, episodes, incidents, and findings accrue over
  minutes.
- **Optional `--profile heavy`** populates the exotic-probe folder (mysql, mssql,
  mongo, cassandra, kafka, …) if you want a denser Monitors view in scene 5 — not
  required, and it's resource-hungry.
- **Tight on time?** Scenes 4 and 6 are the most compressible (one chart each).
  Scenes 1, 2, 7, and 9 carry the story — protect those.
- Upload to **YouTube** (unlisted or public), then paste the link into both the
  Devpost form and the repo/deploy description.

---

## What each scene proves to a judge

| Scene | Judging signal |
|---|---|
| 1 | Problem + monetizable B2B framing (replaces a 5-tool stack). |
| 2 | **Required AWS DB usage** — live Aurora PostgreSQL, migrations on boot. |
| 3–4 | Observability breadth, real OTLP / Prometheus / RUM / profiling ingest. |
| 5 | Drop-in Sentry compatibility + uptime → on-call paging, end to end. |
| 6 | Observability **and** SIEM in one product; status pages from real incidents. |
| 7 | **Originality** — genuine multi-tenant isolation, the differentiator. |
| 8 | Breadth recap — five products, one binary, one DB. |
| 9 | Production-readiness + the "why Aurora" technical rationale. |
| 10 | Call to action — repo + live link. |
