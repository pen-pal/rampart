# Deploying Rampart — Vercel (frontend) + AWS (backend) + RDS Postgres

The top-level guide for a cloud deploy: the **React SPA on Vercel**, the
**Rampart binary on AWS**, and **managed Postgres on RDS / Aurora**. It ties the
pieces together and links out to the click-by-click detail.

- **Backend + DB, step by step:** [`deploy/aws-vercel.md`](deploy/aws-vercel.md) — RDS/Aurora setup, App Runner / ECS Fargate / EC2 options, the rewrite-vs-CORS rationale (verified against source), screenshots checklist.
- **Single-box / Docker-Compose path** (no Vercel, no AWS): [`deploy/README.md`](deploy/README.md) — systemd unit, backups, reverse proxy, custom domains.
- **Kubernetes:** [`KUBERNETES.md`](KUBERNETES.md) — the Helm chart.

If you only want it running locally, `docker compose up -d` from the repo root is
still the zero-config path — this doc is for putting it on the internet.

---

## Architecture

```
Browser ──▶ Vercel (Rampart React SPA + same-origin /v1 rewrite)
              │   browser only ever talks to your-app.vercel.app
              │   Vercel proxies /v1/* server-side → no CORS, cookie flows
              ▼
         AWS API origin  (Rampart Rust binary, port 3000)
              │   App Runner  ·  ECS Fargate + ALB  ·  EC2 + docker
              │   sqlx pool, sslmode=require
              ▼
         RDS / Aurora PostgreSQL  (migrations run on boot)
```

Two facts shape this topology — both verified in source, not assumed:

1. **The SPA calls the API with relative paths.** `frontend/src/lib/api.js` does
   `fetch('/v1/...')` with `credentials: 'same-origin'`, and `sse.js` opens
   `new EventSource('/v1/stream/heartbeats', { withCredentials: true })`. There
   is **no `VITE_API_BASE`** or any API-base-URL env the code reads. The deployed
   SPA reaches the backend **only** via a same-origin rewrite.
2. **CORS is `allow_origin(Any)` without `allow_credentials`** (an intentional
   security invariant in `backend/crates/rampart-api/src/lib.rs`). A cross-origin
   `your-app.vercel.app → api.example.com` call therefore can **not** carry the
   session cookie — auth would silently fail. The Vercel rewrite keeps the
   browser same-origin, so the cookie flows and **no code change is needed.**

> **Why the backend can't live on Vercel.** Rampart is a long-lived process
> (scheduler + ingest listeners + notifier + Postgres advisory-lock leader
> election), not a serverless function. It needs AWS (or any always-on host).

---

## Part 1 — Backend on AWS

The published image is **`ghcr.io/pen-pal/rampart:latest`** (pin a tag, e.g.
`:0.156.79` — GHCR tags strip the leading `v`). It runs migrations on boot and
serves `/healthz` + `/readyz` on port **3000**. The image is built by the
repo-root [`Dockerfile`](../Dockerfile) (frontend → Rust → debian-slim) — you
only build it yourself if you've changed source.

**Recommended for a deadline: AWS App Runner** — a container with an HTTPS
endpoint and autoscaling, no load-balancer to wire. Caveat: it needs a **VPC
connector** to reach a private RDS/Aurora cluster, and it pulls from **ECR, not
GHCR**, so mirror the image first. Full walkthrough (plus ECS Fargate and EC2
alternatives) in [`deploy/aws-vercel.md` §2](deploy/aws-vercel.md). A minimal
App Runner service config skeleton is in [`/deploy/apprunner.yaml`](../deploy/apprunner.yaml).

The database is RDS PostgreSQL or Aurora PostgreSQL — both are wire-compatible,
so it's a connection-string swap, not a rewrite. sqlx reads `sslmode` (and
`sslrootcert`) straight out of the URL, so `…?sslmode=require` just works. See
[`deploy/aws-vercel.md` §1](deploy/aws-vercel.md) for the RDS/Aurora setup.

```
DATABASE_URL=postgres://rampart:STRONG-PW@your-instance.xxxx.us-east-1.rds.amazonaws.com:5432/rampart?sslmode=require
```

### First admin user

Normal boot does **not** auto-create an admin. The **first visit** to the UI
shows a signup form when zero users exist, and the first user becomes admin —
this is the simplest path on App Runner (which has no container exec).

To create the admin non-interactively instead (CI, or a clean handoff), run the
seeder once with the admin env vars set — it creates the admin only when the
users table is empty, so it's safe:

```bash
docker run --rm \
  -e DATABASE_URL='postgres://rampart:PW@<endpoint>:5432/rampart?sslmode=require' \
  -e RAMPART_ADMIN_EMAIL='admin@example.com' \
  -e RAMPART_ADMIN_PASSWORD='a-strong-password' \
  ghcr.io/pen-pal/rampart:0.156.79 rampart-api seed-demo
```

(`seed-demo` also fills every tier with `[demo]` data — see [`DEMO.md`](DEMO.md).
If you want the admin but not the demo rows, just use the signup form.)

---

## Part 2 — Frontend on Vercel

The SPA lives in `frontend/`. The deploy artifact is
[`frontend/vercel.json`](../frontend/vercel.json) — already committed, with
`api.YOURDOMAIN` placeholders you replace with your AWS API origin:

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

Deploy it (CLI shown; dashboard import works too — set **Root Directory =
`frontend`**, framework **Vite**):

```bash
cd frontend
npm i -g vercel
vercel link            # create/link a project; pick your team
vercel --prod
```

Notes that are easy to get wrong:

- **`/v1/*` is everything the SPA fetches** — `api.js` only ever hits `/v1/...`.
  `/push/*` is there only if you want to fire push-monitor heartbeats at the
  Vercel origin; `/healthz` + `/readyz` mirror the dev proxy.
- **Do NOT proxy the telemetry ingest paths** (`/otlp`, `/rum`, `/prom`,
  `/profiles`, Sentry DSN) through Vercel. Point your exporters/SDKs **directly
  at the AWS API origin** — that avoids Vercel function timeouts on large /
  streaming payloads, and the browser SPA never calls those paths anyway.
- **No SPA catch-all rewrite is needed.** The Rampart SPA is a **hash router**
  (`#/...`), so every route already resolves to `/`. (Only if it ever became a
  path router would you add `{ "source": "/(.*)", "destination": "/index.html" }`
  *after* the API rewrites.)

---

## Environment variable reference (backend)

Set these on the AWS service (App Runner config / ECS task def / EC2 `-e`). Put
`DATABASE_URL` and `RAMPART_SECRET_KEY` in **Secrets Manager**, not plaintext.

| Variable | Required | Default | What it does |
| :--- | :--- | :--- | :--- |
| `DATABASE_URL` | **yes** | `postgres://…@localhost` | Backing-store URL; scheme picks the store. For AWS: `postgres://…?sslmode=require`. |
| `RAMPART_SECRET_KEY` | recommended | _(unset)_ | 32-byte hex (`openssl rand -hex 32`). Enables AES-256-GCM encryption-at-rest for channel/SMTP secrets. Set it for the security story. |
| `RAMPART_REQUIRE_SECRET_KEY` | no | _(unset)_ | `1`/`true`/`yes` → process refuses to start without a valid `RAMPART_SECRET_KEY`. Belt-and-suspenders for prod. |
| `BIND_ADDR` | no | `0.0.0.0:3000` | Leave as the image default so the platform health check reaches port 3000. |
| `DATABASE_POOL_SIZE` | no | `16` | Max pool connections. Ensure RDS allows `pool_size × replicas`. |
| `RUST_LOG` | no | `rampart=info,tower_http=warn,info` | Tracing filter. |
| `RAMPART_LOG_FORMAT` | no | _(human)_ | `json` → structured logs for CloudWatch / aggregators. |
| `RAMPART_TRUSTED_PROXIES` | **if behind a proxy/LB** | _(unset)_ | Comma-separated IPs/CIDRs of the LB so `X-Forwarded-For` is honored for rate-limit + audit IPs. Behind an ALB/App Runner you must set this (to a **specific** IP/`/32`) or per-client limits collapse to the proxy. |
| `RAMPART_RLS` | no | _(unset)_ | `1` → Postgres row-level security for tenant isolation at the DB layer (defense-in-depth; Postgres only). |
| `RAMPART_MULTI_ORG` | no | _(unset)_ | `1`/`true` → enforce multi-org tenancy on ingest: a telemetry payload with no matching ingest key is rejected (401) instead of falling back to the default org. Leave unset for a single-tenant deploy. |
| `RAMPART_SSRF_BLOCK_PRIVATE` | no | _(guard on)_ | Controls the outbound-probe SSRF guard that blocks cloud-metadata / private ranges. Leave at the secure default in prod. |
| `RAMPART_ADMIN_EMAIL` / `RAMPART_ADMIN_PASSWORD` | no | _(unset)_ | Only read by the `seed-demo` subcommand: create the first admin non-interactively when the users table is empty (see Part 1). |

SMTP for status-page subscribers is configured **inside the app** at
`/#/settings/smtp`, not via env.

---

## Smoke test (end to end)

After both halves are up, verify the whole path:

1. **Backend reachable + DB live.** `/readyz` returns 200 only when the Postgres
   pool can serve a query:
   ```bash
   curl -fsS https://api.YOURDOMAIN/readyz && echo OK
   curl -s   https://api.YOURDOMAIN/healthz   # also shows version + secrets_at_rest
   ```
2. **Migrations actually ran in RDS** (undeniable proof the schema is in AWS):
   ```bash
   psql "$DATABASE_URL" -c "select count(*) from _sqlx_migrations;"
   ```
3. **Vercel SPA loads and talks to AWS.** Open the Vercel URL → you get the
   Rampart login (or first-run signup). Log in → the dashboard loads. In DevTools
   → Network, `/v1/...` requests return **200 from `your-app.vercel.app`** (the
   Vercel rewrite proxying to AWS). If you see CORS errors, `vercel.json` isn't
   at the project root / the rewrite didn't apply.
4. **Live stream works** (SSE through the rewrite): with the dashboard open, the
   heartbeat strip updates without a manual refresh — confirms
   `/v1/stream/heartbeats` is proxying with the cookie attached.

---

## Decisions still on the operator

- **App Runner vs ECS Fargate vs EC2** — App Runner is least-ops and recommended
  for a deadline; Fargate+ALB is the most "AWS-native"; EC2+compose is the most
  familiar. All three are documented in [`deploy/aws-vercel.md` §2](deploy/aws-vercel.md).
- **RDS vs Aurora** — either works (connection-string swap). Aurora Serverless v2
  scales to near-zero between demo sessions; a single `db.t4g.micro` RDS instance
  is cheaper for an always-small workload.
- **Custom domain** — optional. Without one, use the Vercel-assigned URL and the
  App Runner / ALB hostname directly. With one, put the API domain in
  `vercel.json` and add `RAMPART_TRUSTED_PROXIES`.
- **`RAMPART_SECRET_KEY` for the demo** — see the note in
  [`deploy/aws-vercel.md` §2](deploy/aws-vercel.md) about an upstream flip-path
  delivery quirk when a key is set; for a real deploy you should set it.
