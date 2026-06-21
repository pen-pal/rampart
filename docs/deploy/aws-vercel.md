# Deploy Rampart on AWS (Aurora PostgreSQL) + Vercel — H0 hackathon path

Companion to [`../HACKATHON.md`](../HACKATHON.md). That doc is the submission
playbook; **this** doc is the click-by-click deploy guide it references. It gets
Rampart running with the exact topology the H0 rules require:

```
Browser ──▶ Vercel (frontend: v0/Next.js shell + Rampart React SPA, /v1 proxied)
              │  (same-origin rewrite → no CORS)
              ▼
         AWS API origin (Rampart Rust binary: ECS Fargate / App Runner / EC2)
              │  sqlx pool, sslmode=require
              ▼
         Aurora PostgreSQL (writer endpoint; migrations run on boot)
```

Everything below is verified against the Rampart source — see the "Why no code
change" notes. Nothing here requires touching product code; the *only* deploy
artifact you author is a `vercel.json` and a v0 shell project.

---

## 0. Accounts + prerequisites (do these first)

| Need | Where | Notes |
| --- | --- | --- |
| AWS account | <https://aws.amazon.com> | Request the H0 AWS credits via the Devpost/hackathon form early — provisioning Aurora bills by the hour. |
| Vercel account | <https://vercel.com> | Free hobby tier is fine. Note your **Team ID** (Settings → General). |
| v0 account | <https://v0.app> | For the Next.js landing/login shell. Same login as Vercel. |
| `aws` CLI | `brew install awscli` / `apt install awscli` | Optional — console works too. `aws configure` with your access keys. |
| Local Docker | already have it | Only needed if you want to smoke-test against Aurora before deploying. |

The published backend image is **`ghcr.io/pen-pal/rampart:latest`** (pin a tag,
e.g. `ghcr.io/pen-pal/rampart:0.150.5` — GHCR tags strip the leading `v`). It is
public; no auth needed to pull. It already runs migrations on boot and exposes
`/healthz` + `/readyz` on port **3000**.

---

## 1. Aurora PostgreSQL cluster

Rampart is Postgres-native (sqlx, compile-checked queries, 150+ migrations).
Aurora PostgreSQL is wire-compatible, so this is a **connection-string swap, not
a rewrite**.

### Why no code change (verified)

`backend/crates/rampart-db/src/lib.rs::connect()` passes the `DATABASE_URL`
straight to `sqlx`'s `PgPoolOptions::connect(database_url)`. sqlx parses
`sslmode` (and `sslrootcert`, etc.) out of the URL itself — there is no
hard-coded TLS mode or host assumption in Rampart. So `…?sslmode=require`
"just works." Migrations apply via `sqlx::migrate!("../../migrations")` on every
boot (`migrate()`), idempotent and forward-only.

### Option A — AWS Console (fastest for a one-off)

1. **RDS → Create database.**
2. Engine: **Aurora (PostgreSQL Compatible)**. Pick a recent 15.x/16.x version.
3. Templates: **Dev/Test** (or **Production** if you want Multi-AZ for the
   failover talking point in the video).
4. Settings:
   - DB cluster identifier: `rampart`
   - Master username: `rampart`
   - Master password: generate a strong one, save it.
5. Instance: **Serverless v2** (cheapest, scales to near-zero between demo
   sessions) — set min 0.5 ACU, max 2 ACU. Or a `db.t4g.medium` provisioned
   instance.
6. Connectivity:
   - VPC: the **same VPC** your backend (ECS/App Runner/EC2) will live in.
   - Public access: **No** (keep it private; the backend reaches it inside the
     VPC). For a quick demo you *may* set **Yes** + restrict the security group
     to your IP, but private is the right answer for the writeup.
   - Create/choose a security group that allows **inbound TCP 5432 from the
     backend's security group** (not 0.0.0.0/0).
7. Additional config → **Initial database name: `rampart`** (so the
   `…/rampart` in the URL resolves; otherwise create it manually later).
8. Create. Wait ~5–10 min for status **Available**.
9. Copy the **Writer endpoint** (Connectivity & security tab) — looks like
   `rampart.cluster-xxxx.us-east-1.rds.amazonaws.com`.

### Option B — AWS CLI

```bash
aws rds create-db-cluster \
  --db-cluster-identifier rampart \
  --engine aurora-postgresql \
  --engine-version 16.4 \
  --master-username rampart \
  --master-user-password 'CHANGE-ME-STRONG' \
  --database-name rampart \
  --serverless-v2-scaling-configuration MinCapacity=0.5,MaxCapacity=2 \
  --vpc-security-group-ids sg-xxxxxxxx \
  --db-subnet-group-name your-db-subnet-group

aws rds create-db-instance \
  --db-instance-identifier rampart-1 \
  --db-cluster-identifier rampart \
  --engine aurora-postgresql \
  --db-instance-class db.serverless

# Then fetch the writer endpoint:
aws rds describe-db-clusters --db-cluster-identifier rampart \
  --query 'DBClusters[0].Endpoint' --output text
```

### The DATABASE_URL

```
DATABASE_URL=postgres://rampart:STRONG-PASSWORD@rampart.cluster-xxxx.us-east-1.rds.amazonaws.com:5432/rampart?sslmode=require
```

- `sslmode=require` encrypts the connection without verifying the CA. To verify
  the chain instead, download the Aurora CA bundle and append
  `&sslmode=verify-full&sslrootcert=/path/rds-ca.pem` (sqlx reads both from the
  URL — still no code change).
- URL-encode any special chars in the password (`@`, `/`, `:`, `#`).

### Confirm migrations ran on boot

After the backend is up (next section), check its logs — you'll see the
`sqlx::migrate` lines and then `listening on 0.0.0.0:3000`. Or hit the readiness
probe, which returns 200 only when the DB is reachable:

```bash
curl -fsS https://api.YOURDOMAIN/readyz && echo OK
```

Or inspect the DB directly:

```bash
psql "$DATABASE_URL" -c "select count(*) from _sqlx_migrations;"
psql "$DATABASE_URL" -c "\dt" | head   # 150+ tenant + system tables
```

### The AWS-DB-usage screenshot (required asset)

Capture **both**, side by side or as two images:

1. **RDS console → your `rampart` Aurora PostgreSQL cluster** → the page that
   shows Engine = *Aurora PostgreSQL*, status *Available*, the writer endpoint,
   and the **Monitoring** tab with live connection/CPU graphs (proves it's
   actually serving traffic, not just provisioned).
2. **Your backend's env / config** showing `DATABASE_URL=postgres://…@<cluster
   endpoint>…?sslmode=require` — **redact the password**. (ECS task-definition
   env panel, App Runner config, or a terminal `env | grep DATABASE_URL` with
   the password blanked.)

A third nice-to-have: a `psql` screenshot of `select count(*) from
_sqlx_migrations;` against the Aurora endpoint — undeniable proof the app's
schema lives in Aurora.

---

## 2. Backend on AWS (the Rampart Rust binary)

Vercel can host the frontend but **not** the backend — Rampart is a long-lived
process (ingest listeners + scheduler + notifier + leader election), not a
serverless function. Run the GHCR image in the **same VPC** as Aurora. Three
options, easiest first.

### Required env (all three options)

| Var | Value | Notes |
| --- | --- | --- |
| `DATABASE_URL` | the Aurora URL from §1 | required |
| `RAMPART_SECRET_KEY` | `openssl rand -hex 32` | enables AES-256-GCM secrets-at-rest for channel creds; set it for the security story |
| `BIND_ADDR` | `0.0.0.0:3000` | image default; leave as is so the platform's health check can reach it |
| `RUST_LOG` | `rampart=info,tower_http=warn,info` | image default |
| `RAMPART_LOG_FORMAT` | `json` | structured logs for CloudWatch |

> Note on `RAMPART_SECRET_KEY` + the live demo: the `examples/everything` README
> documents an upstream flip-path bug where, *with a key set*, monitor-flip
> deliveries fail `missing field url` while `/test`/digest/scheduled paths work.
> For a **production** AWS deploy you should set the key (it's the correct,
> secure default). For the **demo video** you can either set it and demo
> `/test`-fired deliveries + digests, or leave it unset to show live flip-path
> deliveries — your call; the readiness section in HACKATHON.md flags this.

### Option A — App Runner (least ops; good for the deadline)

App Runner runs a container with an HTTPS endpoint and autoscaling, no
load-balancer wiring. Caveat: it needs a **VPC connector** to reach a private
Aurora cluster.

1. App Runner → Create service → **Container registry → Public** →
   `public.ecr.aws/...` — note App Runner pulls from ECR, not GHCR directly.
   **Mirror the image to ECR first:**
   ```bash
   aws ecr create-repository --repository-name rampart
   docker pull ghcr.io/pen-pal/rampart:0.150.5
   docker tag  ghcr.io/pen-pal/rampart:0.150.5 \
     <acct>.dkr.ecr.us-east-1.amazonaws.com/rampart:0.150.5
   aws ecr get-login-password | docker login --username AWS --password-stdin \
     <acct>.dkr.ecr.us-east-1.amazonaws.com
   docker push <acct>.dkr.ecr.us-east-1.amazonaws.com/rampart:0.150.5
   ```
2. Service settings: port **3000**, the env vars above (put `DATABASE_URL` +
   `RAMPART_SECRET_KEY` in **Secrets** via Secrets Manager, not plaintext).
3. **Networking → Outgoing traffic → Custom VPC** → add a VPC connector in
   Aurora's VPC/subnets so it can reach the DB security group.
4. Health check path: `/readyz`.
5. Deploy. App Runner gives you `https://xxxx.us-east-1.awsapprunner.com` — that
   is your **API origin** for the Vercel rewrite.

### Option B — ECS Fargate (most "AWS-native" for the writeup)

1. Mirror the image to ECR (as above).
2. **Task definition:** one container, image = your ECR tag, port 3000,
   env = the table above (secrets via Secrets Manager). CPU 512 / mem 1024 is
   plenty for a demo.
3. **Service:** Fargate launch type, in Aurora's private subnets, with a
   security group allowed to reach Aurora's SG on 5432.
4. Front it with an **Application Load Balancer**: target group → port 3000,
   health check path `/readyz`, listener on 443 with an ACM cert. The ALB DNS
   name (or your custom domain on it) is the **API origin**.

### Option C — EC2 + docker compose (most familiar)

1. Launch an EC2 instance (t3.small) in Aurora's VPC; SG allows 443 from the
   internet and is allowed into Aurora's SG on 5432.
2. Install Docker, then run the published image directly:
   ```bash
   docker run -d --name rampart -p 3000:3000 \
     -e DATABASE_URL='postgres://rampart:PASS@<cluster-endpoint>:5432/rampart?sslmode=require' \
     -e RAMPART_SECRET_KEY="$(openssl rand -hex 32)" \
     -e RAMPART_LOG_FORMAT=json \
     --cap-drop ALL --cap-add NET_RAW --security-opt no-new-privileges:true \
     ghcr.io/pen-pal/rampart:0.150.5
   ```
3. Put Caddy/nginx in front for TLS (see `README.md` in this dir). The public
   HTTPS hostname is your **API origin**.

### First-run admin

Whichever option, the **first visit** to the UI shows a signup form (only when
zero users exist); the first user becomes admin. To script it instead:

```bash
# ECS: aws ecs execute-command … ; App Runner: no exec — use the signup form;
# EC2:
docker exec rampart rampart-api reset-password admin@example.com 'your-password'
```

### (Optional) Seed the demo data into Aurora

If you want the dashboard populated for screenshots without wiring the full live
stack, run the seeder against Aurora once:

```bash
# EC2 example (App Runner has no exec; run from any box that can reach Aurora):
docker run --rm \
  -e DATABASE_URL='postgres://rampart:PASS@<cluster-endpoint>:5432/rampart?sslmode=require' \
  ghcr.io/pen-pal/rampart:0.150.5 rampart-api seed-demo
```

---

## 3. Frontend on Vercel

The Rampart operator console is a Vite/React SPA. It calls the API with
**relative paths** (`fetch('/v1/...')`, `credentials: 'same-origin'` — verified
in `frontend/src/lib/api.js`). There is **no `VITE_API_BASE`** env in the code.

### Why the rewrite, not CORS (verified — this is load-bearing)

Rampart's CORS layer is `allow_origin(Any)` **without** `allow_credentials`
(intentional security invariant — `backend/crates/rampart-api/src/lib.rs:52`).
Browsers will **not** send the session cookie on a cross-origin request unless
the server sets `Access-Control-Allow-Credentials: true` *and* a specific
origin. So a Vercel frontend calling the AWS API as a *different origin* cannot
authenticate — the cookie never goes. Two ways out:

- **Add a `VITE_API_BASE` + switch the API to credentialed CORS** → requires
  *product code changes* on both sides. Out of scope for docs-only prep.
- **Vercel same-origin rewrite** → the browser only ever talks to
  `your-app.vercel.app`; Vercel proxies `/v1/*` server-side to the AWS origin.
  Same-origin means the cookie flows normally, **zero code change, zero CORS.**
  ✅ This is the path.

### `vercel.json`

Put this at the root of the Vercel project (the SPA project). Replace
`https://api.YOURDOMAIN` with the API origin from §2 (the App Runner URL, the
ALB hostname, or your EC2 domain).

```json
{
  "buildCommand": "npm ci && npm run build",
  "outputDirectory": "dist",
  "rewrites": [
    { "source": "/v1/:path*",       "destination": "https://api.YOURDOMAIN/v1/:path*" },
    { "source": "/healthz",         "destination": "https://api.YOURDOMAIN/healthz" },
    { "source": "/readyz",          "destination": "https://api.YOURDOMAIN/readyz" },
    { "source": "/push/:path*",     "destination": "https://api.YOURDOMAIN/push/:path*" }
  ]
}
```

Notes:
- `/v1/*` is everything the SPA actually fetches (verified: `api.js` only ever
  hits `/v1/...`). `/push/*` is included for push-monitor heartbeats if you want
  to demo them from the Vercel origin; `/healthz` + `/readyz` mirror the dev
  proxy.
- **The ingest paths (`/otlp`, `/rum`, `/prom`, `/profiles`, Sentry DSN) should
  NOT be proxied through Vercel.** Customer apps (and the `examples/everything`
  stack) point their exporters **directly at the AWS API origin**, not at the
  Vercel domain — that's the correct topology and avoids Vercel function
  timeouts on streaming/large payloads. (The earlier draft listed them; drop
  them from the rewrite — the browser SPA never calls them.)
- SPA fallback: Vite SPAs need unknown routes to serve `index.html`. The
  Rampart SPA is a **hash router** (`#/...`), so all routes already resolve to
  `/` — no catch-all rewrite needed. If you ever switch to a path router, add
  `{ "source": "/(.*)", "destination": "/index.html" }` *after* the API
  rewrites.

### Deploy the SPA (two ways)

**Vercel CLI** (fastest):

```bash
cd frontend
npm i -g vercel
vercel link            # create/link a project; pick your team
# Put the vercel.json above in frontend/ (root of this project)
vercel --prod
```

**Vercel dashboard:** New Project → import the repo → **Root Directory =
`frontend`** → Framework preset **Vite** → Build `npm run build`, Output `dist`
→ add the `vercel.json` (committed in `frontend/`) → Deploy.

### The Project Link + Team ID (required submission fields)

- **Project Link** = the production URL Vercel prints, e.g.
  `https://rampart-console.vercel.app`. Also visible under the project's
  Deployments. This is what you paste into Devpost as the "published Vercel
  project link."
- **Team ID** = Vercel → **Settings → General → Team ID** (or Account ID for a
  personal/hobby account). Copy the `team_xxxxx` value.

### Smoke test the deployed frontend

1. Open the Vercel URL → you should get the Rampart login (or first-run signup).
2. Log in → the dashboard loads → DevTools Network tab shows `/v1/...` requests
   returning 200 from `your-app.vercel.app` (proxied to AWS). If you see CORS
   errors, the rewrite isn't in place / `vercel.json` isn't at the project root.

---

## 4. v0-scaffolded Next.js shell (the v0 requirement)

The rules want a v0-scaffolded Next.js frontend. Strategy: keep the dense,
proven operator console as the React SPA, and have **v0 generate a Next.js
landing/login marketing shell** that links into it.

1. In v0 (<https://v0.app>), prompt for a Next.js landing page:
   *"A dark, technical SaaS landing page for 'Rampart' — a self-hosted,
   multi-tenant observability + SIEM platform. Hero with tagline 'One platform.
   Every signal. Your infrastructure.', a feature grid (uptime, tracing, logs,
   metrics, RUM, profiling, errors, on-call, status pages, SIEM detections), a
   'Sign in' CTA button, and a footer. Built on Aurora PostgreSQL, deployed on
   Vercel."*
2. v0 → **Deploy to Vercel** (one click) — this creates the Next.js project on
   Vercel. The **Sign in** CTA links to the operator-console project URL from §3
   (or to a `/app` route you rewrite to it).
3. You now have two Vercel projects (shell + console) or one (if you embed). For
   the submission, the **shell is the headline "published Vercel project link"**;
   mention the console SPA as the deployed app it fronts. Both count as Vercel
   deploys.

Document the v0 project URL in the Devpost writeup ("frontend shell scaffolded
with v0, deployed on Vercel").

---

## 5. Optional: live demo data on the deployed stack

For the **video**, the richest footage comes from the `examples/everything`
stack (real OTLP/RUM/Sentry/Prometheus/flapping monitors/2-org RLS isolation).
Two ways to use it for the recording:

- **Record locally** (simplest): run `cd examples/everything && docker compose
  up` on your laptop, point the screen-capture at `http://localhost:3000`. The
  AWS+Vercel+Aurora deploy is shown via the *architecture diagram + Aurora
  console shot*; the feature tour is the local everything-stack. This is the
  pragmatic split most submissions use and is fully honest as long as you show
  the Aurora console live.
- **Record against the deployed stack** (strongest): point the
  `examples/everything` real services (demo-app OTLP exporter, Prometheus
  remote_write, Sentry DSN, RUM snippet) at the **AWS API origin** instead of
  the in-compose `rampart` service, with the deployed instance on Aurora. More
  wiring, but then every tier you show is genuinely flowing into Aurora — the
  most defensible demo. Set the exporters' endpoints to `https://api.YOURDOMAIN`
  and provision config via the API against that origin.

Either way: `bash examples/everything/verify.sh` asserts every tier is
non-empty before you record.

---

## 6. Quick reference — what proves what

| Submission requirement | Proof artifact | Where it comes from |
| --- | --- | --- |
| Uses Aurora PostgreSQL | Aurora console screenshot + `DATABASE_URL` (redacted) + `_sqlx_migrations` count | §1 |
| Frontend on Vercel | Project Link + Team ID | §3, §4 |
| Architecture diagram | PNG/SVG export of the ASCII in HACKATHON.md §3 | HACKATHON.md |
| <3-min demo video | YouTube unlisted/public link | HACKATHON.md shot list |
| Backend actually runs on the DB | `/readyz` returns 200 (DB-gated) | §1, §2 |
