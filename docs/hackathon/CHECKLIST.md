# Rampart — Devpost Submission Checklist

> Field-by-field readiness for the H0 Devpost entry (<https://h01.devpost.com/>,
> deadline **2026-06-29 17:00 PDT**, track **Monetizable B2B App**).
> Legend: ✅ ready in-repo now · ⬜ needs owner (account / deploy / recording / UI).
>
> The paste-ready copy is in [`SUBMISSION.md`](SUBMISSION.md). Deploy mechanics:
> [`../DEPLOY.md`](../DEPLOY.md) + [`../deploy/aws-vercel.md`](../deploy/aws-vercel.md).
> The zero-to-live runbook is [`GO_LIVE.md`](GO_LIVE.md); the demo-video script +
> screenshot list is [`DEMO_SCRIPT.md`](DEMO_SCRIPT.md). Everything below maps to
> shipped code at workspace **v0.157.12**.

---

## Required Devpost fields

| Field | State | Source / note |
|---|---|---|
| Project name — **Rampart** | ✅ | — |
| Tagline | ✅ | `SUBMISSION.md` → Tagline |
| Elevator pitch | ✅ | `SUBMISSION.md` → Elevator pitch |
| Inspiration | ✅ | `SUBMISSION.md` |
| What it does | ✅ | `SUBMISSION.md` |
| How we built it | ✅ | `SUBMISSION.md` |
| Challenges we ran into | ✅ | `SUBMISSION.md` |
| Accomplishments we're proud of | ✅ | `SUBMISSION.md` |
| What we learned | ✅ | `SUBMISSION.md` |
| What's next for Rampart | ✅ | `SUBMISSION.md` |
| Built with (tech tags) | ✅ | `SUBMISSION.md` → Built with |
| "Which AWS Database" answer | ✅ | `SUBMISSION.md` → Aurora PostgreSQL block |
| Public repo link | ✅ copy / ⬜ confirm | `https://github.com/pen-pal/rampart` — **confirm it is public before paste** |
| Track selected (Monetizable B2B App) | ⬜ | Set when creating the Devpost project |
| Screenshots gallery | ⬜ | Feature PNGs exist in `site/assets/screenshots/`; the org-switcher (multi-tenant) and optional `sqlite:`-boot shots must be captured live |
| Architecture diagram (PNG/SVG) | ⬜ | Mermaid + ASCII source in `SUBMISSION.md` / `../DEPLOY.md`; export to an image |
| Demo video (YouTube, < 3 min) | ✅ script / ⬜ record | Shot list in [`DEMO_SCRIPT.md`](DEMO_SCRIPT.md); record + upload |
| Live demo URL (Vercel) | ⬜ | Produced by the deploy — follow [`GO_LIVE.md`](GO_LIVE.md) |
| Vercel Project Link | ⬜ | Captured at deploy time |
| Vercel Team ID | ⬜ | Vercel → Settings → General → Team ID |
| AWS-DB-usage screenshot | ⬜ | Aurora console + redacted `DATABASE_URL` |
| Team / submitter | ⬜ | Devpost account + any teammates |
| SUBMITTED before deadline | ⬜ | Leave ≥ 1 day buffer before 2026-06-29 17:00 PDT |

## Hackathon qualification requirements

| Requirement | State | Closing action |
|---|---|---|
| AWS DB = **Aurora PostgreSQL** | ✅ wire-compatible, no code change | Provision + set `DATABASE_URL=…?sslmode=require` (`../deploy/aws-vercel.md §1`) |
| **Frontend on Vercel** | ✅ `frontend/vercel.json` rewrite present / ⬜ deploy | Replace `YOURDOMAIN` with the real AWS origin, `vercel --prod` (`../DEPLOY.md`) |
| Use **v0** to scaffold Next.js | ⬜ | Generate a v0 Next.js landing/login shell, one-click deploy (`../deploy/aws-vercel.md §4`) |
| Backend hosting | ✅ image exists / ⬜ deploy | Run `ghcr.io/pen-pal/rampart` on App Runner / ECS / EC2 in Aurora's VPC |

## Product / repo (the thing being judged) — all ✅

- ✅ Full feature set shipped (uptime 42 kinds, traces, logs, metrics, RUM,
  profiling, errors, SIEM, on-call, status pages) in one binary — `CHANGELOG.md`
  through v0.157.12.
- ✅ Pre-submission hardening pass — closed cross-tenant telemetry leaks
  (heartbeat SSE stream, scheduled reports), rate-limited the public status-page
  `/unlock` DoS surface, made bearer auth crash-safe on non-Postgres backends,
  and fixed syslog-parser / incident-dedup / uptime-math correctness bugs, each
  with a named regression test — `CHANGELOG.md` v0.156.84–v0.157.12.
- ✅ Multi-tenancy (orgs, per-org RBAC, per-org ingest creds, org switcher,
  OIDC→org) — Phases 1–5 shipped; `../MULTITENANCY.md`.
- ✅ Postgres RLS tenant isolation (defense-in-depth) — flag-gated `RAMPART_RLS=1`,
  on in the demo stack.
- ✅ Aurora PostgreSQL compatibility — connection-string swap, migrations on boot.
- ✅ Live demo stack proving every tier with real data — `examples/everything` +
  `verify.sh`.
- ✅ Multi-backend behind the `Store` seam — Postgres (reference) + SQLite (full
  monitoring) + MySQL (management-API tier), selected by `DATABASE_URL`.

---

## ⬜ OWNER STILL NEEDS TO PROVIDE

Everything above marked ⬜ requires a human. The hard blockers, grouped:

1. **Devpost account + project.** Create the entry, select track **Monetizable B2B
   App**, add any teammates, paste the copy from `SUBMISSION.md`.
2. **Deploy to produce the live URL + AWS proof.** Follow the numbered runbook in
   [`GO_LIVE.md`](GO_LIVE.md): provision Aurora PostgreSQL (Serverless v2, private
   subnets, SG open to the backend SG on 5432); deploy the backend
   (`ghcr.io/pen-pal/rampart:0.157.12`) on EC2 + `deploy/compose.aws.yaml` (simplest)
   or ECS Express / Fargate with `DATABASE_URL`, `RAMPART_SECRET_KEY`,
   `BIND_ADDR=0.0.0.0:3000`; confirm `/readyz` → 200; set the real API origin in
   `frontend/vercel.json` and `vercel --prod` the SPA. Produces: the live "try it"
   URL, the **Vercel Project Link + Team ID**, and the **AWS-DB console screenshot**.
3. **Scaffold + deploy the v0 Next.js landing/login shell** (satisfies the "use v0"
   requirement).
4. **Record + upload the demo video** (≤ 3:00, YouTube). Follow the scene-by-scene
   script in [`DEMO_SCRIPT.md`](DEMO_SCRIPT.md); record locally against
   `examples/everything` for the feature tour and cut to the live Aurora console
   for the DB proof.
5. **Capture the live screenshots** the gallery still needs: the org-switcher /
   multi-tenant isolation shot, and (optional, high-impact) a terminal showing the
   same binary booting on `DATABASE_URL=sqlite:…`.
6. **Export the architecture diagram** (Mermaid/ASCII in `SUBMISSION.md`) to PNG/SVG.
7. **Confirm the repo is public** before pasting the link.
8. **Request AWS credits** via the H0 form early (Aurora bills hourly).
9. **(Bonus)** Publish a build blog/video with `#H0Hackathon` and the "created for
   this hackathon" statement.

---

## On-camera honesty guardrails (don't violate in the video or copy)

- **Multi-backend:** Postgres + SQLite both run the **full monitoring stack**;
  MySQL serves the **management API** today. Do **not** say "runs on three
  databases for monitoring" or imply MySQL drives the scheduler/alerting. All three
  are opt-in cargo features; the default build is Postgres-only.
- **Tenancy:** isolation is app-layer per-request `org_id` scoping; RLS
  (`RAMPART_RLS`) is opt-in defense-in-depth (ENABLE not FORCE, owner-exempt), on
  only in the demo stack. Do **not** say "RLS enforced everywhere."
- **Migrations:** the safe spoken line is "100+ migrations" unless the exact count
  (118) is on screen.
- **Secrets-at-rest vs. live deliveries:** keep `RAMPART_SECRET_KEY` **unset**
  during the demo so live notification deliveries work (known decrypt issue on the
  flip path when set). Show live deliveries **or** talk about encryption-at-rest —
  never both in one breath.

## Sibling hackathon docs (context, not duplicates)

This `docs/hackathon/` set is the consolidated, current deliverable:

- [`SUBMISSION.md`](SUBMISSION.md) — paste-ready Devpost copy (v0.157.12).
- [`GO_LIVE.md`](GO_LIVE.md) — zero-to-live deploy runbook (Aurora + Vercel).
- [`DEMO_SCRIPT.md`](DEMO_SCRIPT.md) — the < 3-min demo-video shot list.
- this file — field-by-field readiness.

The older top-level docs remain for deeper context (fact-checked at v0.156.x; the
hackathon-folder set above supersedes them where they overlap):

- `../HACKATHON.md` — full runbook + judging-criteria narrative.
- `../HACKATHON_SUBMISSION.md` — earlier paste-ready copy (Mermaid diagram source).
- `../HACKATHON_SUBMISSION_PACKAGE.md` — the fact-check table (every number → source).
- `../HACKATHON_DEMO.md` — the earlier scene-by-scene video script + screenshot list.
