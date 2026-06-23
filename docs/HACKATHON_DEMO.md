# Rampart — Hackathon Demo Delivery Kit

> The **two deliverables** the H0 submission still needs a human to execute: a
> **2–3 minute demo video script** (scene-by-scene, what to show / what to say /
> timings) and a **readiness checklist** (done vs. todo before the
> **2026-06-29 17:00 PDT** deadline).
>
> This is the *execution* doc. The strategy, full Devpost copy, and deploy
> mechanics live elsewhere — don't duplicate them, reference them:
> - Plan / runbook / judging narrative → [`HACKATHON.md`](HACKATHON.md)
> - Paste-ready Devpost write-up → [`HACKATHON_SUBMISSION.md`](HACKATHON_SUBMISSION.md)
> - Deploy mechanics (Aurora / AWS / Vercel / v0) → [`deploy/aws-vercel.md`](deploy/aws-vercel.md)
> - The live demo stack this script films against → [`../examples/everything/README.md`](../examples/everything/README.md)
>
> Everything in the script maps to **shipped** code (workspace at v0.156.0) and a
> tier the `examples/everything` stack genuinely fills with **real** data — no
> fabricated rows, no vaporware.

---

## Part 1 — Demo video script (target 2:50, hard cap < 3:00)

### Before you hit record (5 minutes of prep)

1. `cd examples/everything && docker compose up` (default profile is enough; add
   `--profile heavy` only if you want the exotic-probe folder populated). The
   remote-agent service compiles from source on first `up` — give it a few min.
2. **Let it run ≥ 5 minutes** so monitors flap, episodes open, SLO budgets burn,
   incidents open/close, and detection findings accrue.
3. `bash examples/everything/verify.sh` → must print **`✅ ALL TIERS NON-EMPTY`**.
   Do not record until it's green.
4. Log in at <http://localhost:3000> as `demo@rampart.local` /
   `Rampart-Live-9271`. Pre-warm the tabs you'll click so nothing is mid-load.
5. Have a second browser tab on the **Aurora console** (engine = Aurora
   PostgreSQL, status Available, a Monitoring graph) — this is the AWS-DB proof
   shot. (Record the feature tour locally against the rich `everything` data; cut
   to the live Aurora console for the DB proof. That local/cloud split is honest
   and standard — see `deploy/aws-vercel.md §5`.)
6. Record 1080p+, mouse movements slow and deliberate, no dead air. Scripted
   narration below ≈ 360 words ≈ paced for ~2:50.

> **One on-camera honesty rule** (`HACKATHON.md §6 gap 2`): the published image
> has an upstream bug where, *with `RAMPART_SECRET_KEY` set*, the live
> monitor-flip notification path fails to decrypt. The demo ships with the key
> **unset** so flip-path deliveries are real. So on camera, either show live
> flip-path deliveries (key unset — the default) **or** talk about
> encryption-at-rest, not both in the same breath.

### Scene-by-scene

| # | Time | Show on screen | Say (narration) |
|---|------|----------------|-----------------|
| 1 | 0:00–0:18 | **Title card**, then 5 vendor logos (Datadog, Sentry, PagerDuty, a status-page vendor, a SIEM) crossed out → Rampart logo. | "Engineering and security teams pay for five separate tools — metrics, errors, on-call, a status page, a SIEM. Five bills, five data silos, five logins. Rampart is all of it — one self-hosted, multi-tenant platform, on Aurora PostgreSQL." |
| 2 | 0:18–0:33 | Architecture diagram (`HACKATHON.md §4`) → **cut to live Aurora console**: engine *Aurora PostgreSQL*, status Available, a Monitoring graph. | "A Vercel frontend talks to a single Rust binary on AWS, backed by Aurora PostgreSQL. Here's the live cluster — 118 migrations ran on boot." |
| 3 | 0:33–0:53 | **Traces** view → open the `/api/checkout` trace → the waterfall with the errored leaf span → click the linked log line carrying the same **trace id**. | "Real OpenTelemetry traces from an instrumented app — an Express service through Postgres and Redis. Drill into the errored span, jump straight to the correlated log line by trace id. This is live OTLP, not seed data." |
| 4 | 0:53–1:10 | **Metrics** → the `demo_queue_depth` chart (breaching its rule) → **RUM** → a web-vitals session with a poor LCP → **Profiling** → a folded/flame CPU profile. | "Prometheus remote-write metrics with rule-based alerting, real-user web vitals from an actual browser page, and continuous CPU profiles — all three profiling formats genuinely ingested." |
| 5 | 1:10–1:30 | **Errors** → an issue grouped in a project (users-affected, by-release) → **Monitors** → the `edge · flapping ready probe` showing a Down flip + uptime strip → an **escalation / on-call** episode that paged. | "Sentry-compatible error tracking that groups by release and counts users affected. An uptime monitor genuinely flipping Down — and the escalation policy paging the on-call schedule for real." |
| 6 | 1:30–1:48 | **Status pages** → the public page with an open incident + updates → **Detections** → the `failed login` SIEM rule with raised **findings**. | "A public status page driven by real incidents. And the SIEM side: a detection rule firing on real auth-failure logs and raising findings — observability and security in one product." |
| 7 | 1:48–2:18 | **THE MONEY SHOT.** Org switcher: `Default` → `Demo Team` → its *own* `demo-team ·` monitors + telemetry. Then attempt to open a `Default`-only resource by URL → **404 / not visible**. | "Two tenants, each with their own monitors, telemetry, and ingest credentials. Switch orgs — you only ever see your own. Try to reach the other org's resource and it's gone. Isolation is enforced primarily by per-request `org_id` scoping in the app, with Postgres row-level security enabled here as defense-in-depth." |
| 8 | 2:18–2:40 | Slow pan across the left nav: Uptime, Traces, Logs, Metrics, RUM, Profiling, Errors, On-call, Status, Detections. | "Every tier that's normally five separate products — uptime, tracing, logs, metrics, RUM, profiling, errors, on-call, status pages, detections — in one Rust binary, one Postgres database, one UI." |
| 9 | 2:40–2:55 | Back to the **Aurora Monitoring graph**. | "Observability is write-heavy, query-heavy, and retention-bound — exactly what Aurora PostgreSQL scales, while keeping the relational integrity our routing and tenant isolation depend on." |
| 10 | 2:55–3:00 | **Repo URL + live Vercel link** on screen. | "Self-hosted observability and SIEM, on Aurora. Repo and live demo in the description." |

**Fallback if a tier looks thin on camera:** give the stack more uptime and
re-run `verify.sh`; traces/episodes/incidents/findings accrue over minutes. The
org-switch shot (scene 7) is the originality money shot — budget a re-take so
it's clean.

---

## Part 2 — Readiness checklist (done vs. todo → 2026-06-29)

Legend: **DONE** = shipped/verifiable in-repo now · **TODO** = needs a human
action (account, recording, deploy, UI). Owner-gated items are the long poles.

### A. Product / repo (the thing being judged)

| Item | State | How / where |
|---|---|---|
| Full feature set shipped | **DONE** | Observability (uptime 42 kinds, traces, logs, metrics, RUM, profiling, errors) + SIEM + on-call + status pages, all in one binary — `CHANGELOG.md` through v0.156.0. |
| Multi-tenancy (orgs, per-org RBAC, per-org ingest creds, org switcher) | **DONE** | Phases 1–5 shipped; `docs/MULTITENANCY.md`. |
| Postgres RLS tenant isolation (defense-in-depth) | **DONE** | Flag-gated `RAMPART_RLS=1`; the demo stack runs it on. |
| Aurora PostgreSQL compatibility | **DONE** | sqlx reads `sslmode` from the URL; `migrate()` runs on boot — connection-string swap, no code change. `deploy/aws-vercel.md §1`. |
| Live demo stack proving every tier with real data | **DONE** | `examples/everything` + `verify.sh` asserts every tier non-empty. |
| README / docs polish (root README, SETUP, ARCHITECTURE) | **DONE** | Present and current; final pass before submit is cheap insurance. |
| Repo public + link ready | **TODO** | Confirm <https://github.com/pen-pal/rampart> is public; paste link in Devpost. |
| Final `verify.sh` green run captured | **TODO** | `bash examples/everything/verify.sh` → screenshot/save the `✅ ALL TIERS NON-EMPTY` line as evidence. |

### B. Deploy (required: Aurora DB + frontend on Vercel/v0) 🔴 OWNER

| Item | State | How / where |
|---|---|---|
| AWS / Vercel / v0 accounts + AWS credits requested | **TODO** | `deploy/aws-vercel.md §0`; request credits via the H0 form (Aurora bills hourly — do first). |
| Aurora PostgreSQL cluster provisioned (Serverless v2, private subnets) | **TODO** | `deploy/aws-vercel.md §1`. DB SG allows 5432 **from the backend SG only**. |
| `DATABASE_URL` built with `?sslmode=require` | **TODO** | `deploy/aws-vercel.md §1 (The DATABASE_URL)`. |
| Backend deployed (App Runner / ECS / EC2) with env vars | **TODO** | `deploy/aws-vercel.md §2`. Env: `DATABASE_URL`, `RAMPART_SECRET_KEY=$(openssl rand -hex 32)`, `BIND_ADDR=0.0.0.0:3000`. |
| `/readyz` → 200 (migrations confirmed on boot) | **TODO** | `curl https://api.YOURDOMAIN/readyz`; `deploy/aws-vercel.md §1 (Confirm migrations)`. |
| First admin created on the deploy | **TODO** | Signup form on first visit, or `reset-password`; `deploy/aws-vercel.md §2`. |
| `frontend/vercel.json` same-origin `/v1` rewrite → AWS API | **TODO** | The single load-bearing gotcha — **do not use plain CORS**, it silently breaks the session cookie. `deploy/aws-vercel.md §3`. |
| SPA deployed to Vercel; login + `/v1` return 200 in DevTools | **TODO** | `vercel --prod`, root dir `frontend`, Vite preset; `deploy/aws-vercel.md §3`. |
| v0-scaffolded Next.js landing/login shell deployed | **TODO** | `deploy/aws-vercel.md §4` (satisfies the "use v0" requirement). |
| (Optional) point `examples/everything` exporters at the AWS origin | **TODO** | Makes every tier flow into Aurora live; `deploy/aws-vercel.md §5`. |

### C. Assets (video + screenshots + diagram) 🔴 OWNER

| Item | State | How / where |
|---|---|---|
| Demo video script | **DONE** | Part 1 above (refined from the `examples/everything` reality). |
| < 3-min demo video recorded | **TODO** | Follow Part 1; record locally against `examples/everything`, cut to live Aurora for the DB proof. |
| Video uploaded to YouTube (unlisted/public) + link copied | **TODO** | Paste link into Devpost + the deploy README description. |
| Architecture diagram exported to PNG/SVG | **TODO** | ASCII source in `HACKATHON.md §4`; redraw in asciiflow/Excalidraw/draw.io and export an image. |
| AWS-DB-usage screenshot (Aurora console + redacted `DATABASE_URL`) | **TODO** | `deploy/aws-vercel.md §1 (The AWS-DB-usage screenshot)`; bonus: `_sqlx_migrations` count. |

### D. Devpost form fields 🔴 OWNER

| Field | State | How / where |
|---|---|---|
| Project created, track = **Monetizable B2B App** | **TODO** | `HACKATHON.md §1` for track rationale. |
| "Which AWS Database" answer | **DONE (copy)** | Paste from `HACKATHON_SUBMISSION.md` / `HACKATHON.md §5` — Aurora PostgreSQL. |
| Inspiration / What-it-does / How-built / Challenges / Accomplishments / What's-next | **DONE (copy)** | Paste-ready in `HACKATHON_SUBMISSION.md`. |
| Vercel **Project Link** + **Team ID** | **TODO** | Captured during deploy; `deploy/aws-vercel.md §3 (Project Link + Team ID)`. |
| YouTube demo link | **TODO** | From asset C. |
| Architecture diagram attached | **TODO** | From asset C. |
| AWS-DB screenshot attached | **TODO** | From asset C. |
| Repo link attached | **TODO** | From A. |
| **SUBMITTED** before 2026-06-29 17:00 PDT | **TODO** | Leave ≥ 1 day buffer for a re-record / Aurora SG fix; `HACKATHON.md §0 step 21`. |

### E. Bonus (optional, extra credit)

| Item | State | How |
|---|---|---|
| Build blog/video with `#H0Hackathon` + "created for this hackathon" statement | **TODO** | Publish anywhere; tag and include the required statement. |

---

## At-a-glance: the critical path to submit

1. **Deploy** Aurora + backend + Vercel SPA + v0 shell (section B) — longest pole.
2. **Capture** the AWS-DB screenshot + export the diagram (section C).
3. **Record + upload** the < 3-min video using Part 1 (section C).
4. **Fill + submit** the Devpost form with all links (section D), ≥ 1 day early.

Copy is already written; the remaining work is human actions: deploy, record,
screenshot, paste, submit.
