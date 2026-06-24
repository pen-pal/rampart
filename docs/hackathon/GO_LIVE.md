# Rampart — Go-Live Runbook (zero → public demo)

> A numbered, copy-paste path from nothing to a live public Rampart demo on
> **Aurora PostgreSQL** (the required AWS DB) with the frontend on **Vercel**.
> Every command is pulled from the verified deploy docs — nothing invented.
>
> Source docs (read if a step needs more detail):
> [`../DEPLOY.md`](../DEPLOY.md) · [`../deploy/aws-vercel.md`](../deploy/aws-vercel.md) ·
> [`../../deploy/compose.aws.yaml`](../../deploy/compose.aws.yaml) ·
> [`../../deploy/aws.env.example`](../../deploy/aws.env.example).
> The paste-ready Devpost copy is in [`SUBMISSION.md`](SUBMISSION.md); the video
> shot list in [`DEMO_SCRIPT.md`](DEMO_SCRIPT.md).
>
> Published backend image: **`ghcr.io/pen-pal/rampart`** (public; GHCR tags strip
> the leading `v`). Pin a tag — this runbook uses **`0.157.12`** (the current
> workspace version). It runs migrations on boot and serves `/healthz` + `/readyz`
> on port **3000**.

---

## Which backend host? (pick one — simplest stated)

Vercel can host the SPA but **not** the backend — Rampart is a long-lived process
(ingest listeners + scheduler + notifier + Postgres advisory-lock leader
election), not a serverless function. It runs in the **same VPC** as Aurora.

- **EC2 + docker compose — SIMPLEST, recommended for a deadline.** One box, one
  `docker compose up -d` using the committed [`deploy/compose.aws.yaml`](../../deploy/compose.aws.yaml).
  Fewest moving parts, fully copy-paste. **This runbook uses it.** (Caveat: you
  put a TLS reverse proxy in front, step 3.)
- **Amazon ECS Express Mode** — the AWS-native, least-ops managed path (AWS's
  App Runner replacement; App Runner is closed to new customers). One
  `aws ecs create-express-gateway-service` call provisions Fargate + ALB +
  autoscaling from your ECR image; set container port `3000` and health-check
  path `/readyz`. Use this if you want it managed — details in
  [`../deploy/aws-vercel.md` §2](../deploy/aws-vercel.md).
- **ECS Fargate + ALB** — most "AWS-native" for the writeup; fully manual.
  [`../deploy/aws-vercel.md` §2 Option B](../deploy/aws-vercel.md).

The Vercel + Aurora + smoke-test steps are identical whichever you pick.

---

## 0. Prerequisites (do first — Aurora bills hourly)

| Need | Where | Note |
|---|---|---|
| AWS account + credits | <https://aws.amazon.com> | Request H0 AWS credits via the hackathon form **first** — Aurora bills by the hour. |
| Vercel account | <https://vercel.com> | Free hobby tier is fine. Note your **Team ID** (Settings → General). |
| v0 account | <https://v0.app> | Same login as Vercel; for the Next.js shell (step 6). |
| `aws` CLI + `psql` + `openssl` | local | `aws configure` with your keys. |

---

## 1. Provision Aurora PostgreSQL

Console path (fastest one-off) — full detail in
[`../deploy/aws-vercel.md` §1](../deploy/aws-vercel.md):

1. **RDS → Create database.**
2. Engine **Aurora (PostgreSQL Compatible)**, a recent 15.x/16.x version.
3. DB cluster identifier `rampart`; master username `rampart`; generate + save a
   strong password.
4. Instance **Serverless v2** (min 0.5 ACU, max 2 ACU — scales toward zero
   between demo sessions).
5. Connectivity: the **same VPC** the backend will live in; **Public access: No**;
   a security group allowing **inbound TCP 5432 from the backend's SG** (not
   `0.0.0.0/0`).
6. Additional config → **Initial database name: `rampart`**.
7. Create; wait ~5–10 min for **Available**; copy the **Writer endpoint**.

CLI alternative (`../deploy/aws-vercel.md` §1 Option B):

```bash
aws rds create-db-cluster \
  --db-cluster-identifier rampart \
  --engine aurora-postgresql --engine-version 16.4 \
  --master-username rampart --master-user-password 'CHANGE-ME-STRONG' \
  --database-name rampart \
  --serverless-v2-scaling-configuration MinCapacity=0.5,MaxCapacity=2 \
  --vpc-security-group-ids sg-xxxxxxxx --db-subnet-group-name your-db-subnet-group

aws rds create-db-instance \
  --db-instance-identifier rampart-1 --db-cluster-identifier rampart \
  --engine aurora-postgresql --db-instance-class db.serverless

aws rds describe-db-clusters --db-cluster-identifier rampart \
  --query 'DBClusters[0].Endpoint' --output text   # the writer endpoint
```

**Build the `DATABASE_URL`** (URL-encode `@ / : #` in the password):

```
postgres://rampart:STRONG-PASSWORD@rampart.cluster-xxxx.us-east-1.rds.amazonaws.com:5432/rampart?sslmode=require
```

`sslmode=require` encrypts the connection; `sqlx` reads it straight from the URL —
**no code change**. (For CA verification append
`&sslmode=verify-full&sslrootcert=/path/rds-ca.pem`.)

---

## 2. Deploy the backend (EC2 + docker compose)

1. Launch an EC2 instance (e.g. `t3.small`) in **Aurora's VPC**; its security
   group is **allowed into Aurora's SG on 5432**, and allows **443 from the
   internet** (for the proxy in step 3). Install Docker + the Compose plugin.

2. On the box, clone the repo (you only need `deploy/`) and fill the env file:

   ```bash
   git clone https://github.com/pen-pal/rampart.git
   cd rampart
   cp deploy/aws.env.example deploy/aws.env
   ```

   Edit `deploy/aws.env` — at minimum:

   ```bash
   DATABASE_URL=postgres://rampart:STRONG-PASSWORD@<writer-endpoint>:5432/rampart?sslmode=require
   RAMPART_SECRET_KEY=        # see the note below before setting
   RAMPART_LOG_FORMAT=json
   # Behind the step-3 proxy, set this to the proxy's specific IP (or /32):
   # RAMPART_TRUSTED_PROXIES=127.0.0.1
   ```

   **`RAMPART_SECRET_KEY` decision (load-bearing for the demo):** for a real
   production deploy, set it — `openssl rand -hex 32` — to enable AES-256-GCM
   secrets-at-rest. For the **demo recording**, leave it **unset** so live
   monitor-flip notification deliveries work (the published image has an upstream
   flip-path decrypt quirk when a key is set; see
   [`../deploy/aws-vercel.md` §2](../deploy/aws-vercel.md)). Show live deliveries
   **or** encryption-at-rest, not both.

3. Pin the image tag in the compose file (it defaults to `:latest`) and bring it
   up:

   ```bash
   # in deploy/compose.aws.yaml, set:  image: ghcr.io/pen-pal/rampart:0.157.12
   docker compose --env-file deploy/aws.env -f deploy/compose.aws.yaml up -d
   docker compose -f deploy/compose.aws.yaml logs -f rampart
   ```

   You'll see the `sqlx::migrate` lines then `listening on 0.0.0.0:3000`. The
   compose file already runs the container hardened (drops all caps except
   `NET_RAW` for ICMP, `no-new-privileges`, read-only rootfs, `/readyz`
   healthcheck).

4. Local readiness check (on the box):

   ```bash
   curl -fsS http://127.0.0.1:3000/readyz && echo OK   # 200 only when the DB pool serves a query
   ```

---

## 3. TLS in front of the backend → your **API origin**

The compose file binds plain HTTP on `3000`; put a TLS-terminating reverse proxy
in front (Caddy is the least-config). The public HTTPS hostname it serves is your
**API origin** for the Vercel rewrite — e.g. `https://api.example.com` or, with no
custom domain, the EC2 public DNS behind Caddy's automatic-HTTPS for that host.
Proxy snippets are in [`../deploy/README.md`](../deploy/README.md).

> If you instead chose **ECS Express / Fargate + ALB**, the platform gives you the
> HTTPS endpoint directly (ALB DNS or App Runner-style URL) — that is your API
> origin; skip this step.

---

## 4. First admin user

Normal boot does **not** auto-create an admin. Two paths:

- **Signup form (simplest):** the **first visit** to the UI (once Vercel is up,
  step 5) shows a signup form while zero users exist; the first user becomes admin.
- **Scripted (EC2):**
  ```bash
  docker exec rampart rampart-api reset-password admin@example.com 'your-strong-password'
  ```

---

## 5. Frontend on Vercel (point it at the real API origin)

The SPA calls the API with **relative paths** (`fetch('/v1/...')`,
`credentials: 'same-origin'`) and there is **no `VITE_API_BASE`** env. CORS is
`allow_origin(Any)` **without** `allow_credentials`, so a cross-origin call can't
carry the session cookie. The fix is a Vercel **same-origin `/v1` rewrite** — no
CORS, no code change. (Full rationale: [`../deploy/aws-vercel.md` §3](../deploy/aws-vercel.md).)

1. Edit [`frontend/vercel.json`](../../frontend/vercel.json) — replace the four
   `https://api.YOURDOMAIN` placeholders with your **API origin from step 3**:

   ```json
   {
     "$schema": "https://openapi.vercel.sh/vercel.json",
     "buildCommand": "npm ci && npm run build",
     "outputDirectory": "dist",
     "rewrites": [
       { "source": "/v1/:path*", "destination": "https://api.example.com/v1/:path*" },
       { "source": "/healthz",   "destination": "https://api.example.com/healthz" },
       { "source": "/readyz",    "destination": "https://api.example.com/readyz" },
       { "source": "/push/:path*","destination": "https://api.example.com/push/:path*" }
     ]
   }
   ```

   Do **not** proxy the telemetry ingest paths (`/otlp`, `/rum`, `/prom`,
   `/profiles`, Sentry DSN) through Vercel — exporters point directly at the API
   origin. The SPA is a hash router, so no catch-all rewrite is needed.

2. Deploy (CLI):

   ```bash
   cd frontend
   npm i -g vercel
   vercel link        # create/link a project; pick your team
   vercel --prod      # prints the production URL = your live demo link
   ```

   (Dashboard alternative: New Project → import repo → **Root Directory =
   `frontend`** → framework **Vite** → Deploy.)

3. **Capture the two required Vercel fields:**
   - **Project Link** = the production URL Vercel prints (e.g.
     `https://rampart-console.vercel.app`).
   - **Team ID** = Vercel → **Settings → General → Team ID** (`team_xxxxx`).

---

## 6. v0-scaffolded Next.js shell (the "use v0" requirement)

In [v0.app](https://v0.app), prompt for a dark, technical SaaS landing page for
Rampart with a feature grid and a **Sign in** CTA, then **Deploy to Vercel** (one
click). Point the CTA at the operator-console URL from step 5. The shell is the
headline "published Vercel project link"; the console SPA is the app it fronts.
Full prompt + steps: [`../deploy/aws-vercel.md` §4](../deploy/aws-vercel.md).

---

## 7. Seed the demo data into Aurora

Populate every tier for screenshots without wiring the full live stack — run the
seeder once against Aurora (everything it creates is tagged `[demo]`; idempotent):

```bash
# from the EC2 box (or any host that can reach Aurora):
docker run --rm \
  -e DATABASE_URL='postgres://rampart:PASS@<writer-endpoint>:5432/rampart?sslmode=require' \
  ghcr.io/pen-pal/rampart:0.157.12 rampart-api seed-demo
```

> For the **richest video footage**, record locally against the
> `examples/everything` stack instead (real OTLP / RUM / Sentry / Prometheus /
> flapping monitors / 2-org isolation) and cut to the live Aurora console for the
> DB proof — see [`DEMO_SCRIPT.md`](DEMO_SCRIPT.md) and
> [`../deploy/aws-vercel.md` §5](../deploy/aws-vercel.md). The seed above is for
> populating the *deployed* dashboard for screenshots.

---

## 8. Smoke test (end to end) — gate before recording

From [`../DEPLOY.md`](../DEPLOY.md):

1. **Backend reachable + DB live** (200 only when the Aurora pool serves a query):
   ```bash
   curl -fsS https://api.example.com/readyz && echo OK
   curl -s   https://api.example.com/healthz   # shows version + secrets_at_rest
   ```
2. **Migrations actually ran in Aurora** (undeniable proof the schema is in AWS):
   ```bash
   psql "$DATABASE_URL" -c "select count(*) from _sqlx_migrations;"
   ```
3. **Vercel SPA loads + talks to AWS:** open the Vercel URL → login/signup → the
   dashboard loads. In DevTools → Network, `/v1/...` returns **200 from
   `your-app.vercel.app`** (the rewrite proxying to AWS). CORS errors mean
   `vercel.json` isn't at the project root / the rewrite didn't apply.
4. **Live stream works:** with the dashboard open, the heartbeat strip updates
   without a manual refresh — confirms `/v1/stream/heartbeats` proxies with the
   cookie attached.

---

## 9. Capture the required submission assets

| Asset | How |
|---|---|
| **AWS-DB-usage screenshot** | RDS console → your `rampart` Aurora cluster: engine *Aurora PostgreSQL*, status *Available*, writer endpoint, **Monitoring** tab (live CPU/connections). Plus the backend env showing `DATABASE_URL=postgres://…?sslmode=require` with the **password redacted**. Bonus: the `select count(*) from _sqlx_migrations;` output. |
| **Architecture diagram (PNG/SVG)** | Export the Mermaid block in [`SUBMISSION.md`](SUBMISSION.md) (ASCII fallback in [`../DEPLOY.md`](../DEPLOY.md)) via mermaid.live / draw.io / Excalidraw. |
| **Demo video (≤ 3:00, YouTube)** | Record per [`DEMO_SCRIPT.md`](DEMO_SCRIPT.md); upload unlisted/public; copy the link. |
| **Screenshots gallery** | Feature PNGs exist under `site/assets/screenshots/`; capture the **org-switcher / multi-tenant** shot live, and (high-impact, optional) a terminal of the same binary booting on `DATABASE_URL=sqlite:…`. |
| **Vercel Project Link + Team ID** | From step 5.3. |
| **Live demo URL** | The Vercel production URL from step 5.2. |
| **`verify.sh` green run** | If recording against `examples/everything`, screenshot the `✅ ALL TIERS NON-EMPTY` line as evidence. |

---

## 10. Devpost form — fields to paste (from [`SUBMISSION.md`](SUBMISSION.md))

Create the project, select track **Monetizable B2B App**, then paste:

| Devpost field | Source in `SUBMISSION.md` |
|---|---|
| Project name — **Rampart** | — |
| Tagline | → Tagline |
| Elevator pitch | → Elevator pitch |
| Inspiration | → Inspiration |
| What it does | → What it does |
| How we built it | → How we built it |
| Challenges we ran into | → Challenges we ran into |
| Accomplishments we're proud of | → Accomplishments we're proud of |
| What we learned | → What we learned |
| What's next for Rampart | → What's next for Rampart |
| Built with (tech tags) | → Built with |
| **Which AWS Database** | → "Which AWS Database did you use?" (Aurora PostgreSQL block) |

Then attach the links + assets:

- **Repo:** <https://github.com/pen-pal/rampart> (confirm it is **public** before paste).
- **Live frontend (Vercel Project Link):** from step 5.
- **Vercel Team ID:** from step 5.
- **Demo video (YouTube, < 3 min):** from step 9.
- **Architecture diagram (PNG/SVG):** from step 9.
- **AWS-DB-usage screenshot:** from step 9.

**Submit with ≥ 1 day buffer** before **2026-06-29 17:00 PDT** (leave room for a
re-record or an Aurora SG fix).

---

## Quick reference — what proves what

| Requirement | Proof | Step |
|---|---|---|
| Uses **Aurora PostgreSQL** | Aurora console + redacted `DATABASE_URL` + `_sqlx_migrations` count | 1, 8, 9 |
| **Frontend on Vercel** | Project Link + Team ID | 5 |
| Use **v0** to scaffold Next.js | v0 project URL on Vercel | 6 |
| Backend actually runs on the DB | `/readyz` returns 200 (DB-gated) | 2, 8 |
| < 3-min demo video | YouTube link | 9 |
| Architecture diagram | PNG/SVG export | 9 |
