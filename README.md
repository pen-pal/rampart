# Rampart

A self-hosted uptime monitoring tool for homelabs, indie devs, and small teams. Written in Rust, backed by Postgres, ships as a single binary (the React frontend is embedded). Designed to do the boring monitoring jobs well: HTTP, TCP, DNS, ping, TLS / certificate expiry, domain expiry, push (heartbeat) checks, plus a public status page.

```
rampart/
├── backend/         Rust workspace (axum + sqlx + Postgres)
├── frontend/        Vite + React (4 dashboard views)
├── docs/            Design rationale and architecture
├── CONTRIBUTING.md  How to contribute, scope, conventions
├── LICENSE          AGPL-3.0-or-later
└── README.md        ← you are here
```

## Honest state of things

Read this first so you know what you're getting:

- ✅ **Backend** — workspace structure, full Postgres schema, working scheduler that fires probes on intervals, **HTTP + TCP probes** that actually work, REST CRUD for monitors, summary/history/heartbeats endpoints.
- ✅ **Frontend** — 4 pixel-complete views: Dashboard, Monitor detail, Status page builder, New monitor wizard. Dashboard / Monitor detail / New-monitor wizard are wired to the real API. Status page builder is still on mock data (no public renderer yet).
- ✅ **Combined binary** — `rampart-api` embeds the built React bundle via `rust-embed`; release builds ship as a single executable that serves both API and UI on port 3000.
- ⚠️  **Not yet:** auth (anyone with network access can call the API — top-priority before exposing publicly), notifier crate (channels are in the schema but nothing fans out), **17 of 20** probe runners (the records get created; the probes return `Down "not yet implemented"` until the runner is added).

If you want a fully wired, batteries-included monitor today, run something off-the-shelf. Rampart is a *foundation* — the structure and a working core are here, but several probe runners and the notifier are stubbed. See [`docs/DESIGN.md`](docs/DESIGN.md) for the rationale behind the decisions.

---

## Prerequisites

Install these first:

| Tool | Version | Why |
|---|---|---|
| [Docker](https://docs.docker.com/get-docker/) | any recent | Postgres container |
| [Rust](https://rustup.rs/) | 1.78+ | Backend |
| [Node](https://nodejs.org/) | 20+ | Frontend |
| [`sqlx-cli`](https://crates.io/crates/sqlx-cli) | 0.8+ | Database migrations (optional, the app runs them on boot) |

```bash
# install sqlx-cli (optional)
cargo install sqlx-cli --no-default-features --features rustls,postgres
```

---

## Run it (single-binary, recommended)

One process, port 3000, API + UI:

```bash
# 1. Postgres
cd backend
docker compose up -d postgres
cp .env.example .env                # defaults match the compose file

# 2. Frontend bundle (one-shot, only needed when the UI changes)
cd ../frontend && npm install && npm run build

# 3. Backend (also runs migrations on boot, embeds the bundle)
cd ../backend && cargo run -p rampart-api
```

Open <http://localhost:3000>: `GET /` is the React shell, `/v1/*` is the API, unknown paths fall back to `index.html` so deep links into the SPA work. `cargo build --release` bakes the bundle into the executable — Rampart ships as a single file with no asset paths to wire up.

First build pulls ~250 crates and takes 3–5 minutes. Subsequent builds are seconds.

Verify:

```bash
curl http://localhost:3000/healthz   # {"status":"alive"}
curl http://localhost:3000/readyz    # {"status":"ready"} when DB is reachable
curl http://localhost:3000/metrics   # Prometheus scrape (build info only for now)
```

---

## Run it (split, for fast UI iteration)

If you're working on JSX and want Vite's HMR:

```bash
# Terminal A — backend (no need to rebuild the bundle each time)
cd backend && cargo run -p rampart-api

# Terminal B — frontend dev server on :5173, proxies /v1 → backend
cd frontend && npm run dev
```

Open <http://localhost:5173>. Vite proxies `/v1` and `/healthz` to `:3000`, so `fetch('/v1/...')` from any view just works.

---

## Create monitors

```bash
# HTTP
curl -X POST http://localhost:3000/v1/monitors \
  -H "Content-Type: application/json" \
  -d '{
    "name": "example.com",
    "kind": "http",
    "url":  "https://example.com",
    "interval_seconds": 60,
    "timeout_seconds": 10,
    "accepted_statuses": [200, 204]
  }'

# TCP
curl -X POST http://localhost:3000/v1/monitors -H "Content-Type: application/json" -d '{
  "name": "redis-local", "kind": "tcp",
  "hostname": "localhost", "port": 6379,
  "interval_seconds": 30
}'

# Keyword — response body must contain "operational"
curl -X POST http://localhost:3000/v1/monitors -H "Content-Type: application/json" -d '{
  "name": "status keyword", "kind": "keyword",
  "url":  "https://example.com",
  "config": { "keyword": "Example Domain" }
}'

# JSON query — dotted path + expected value
curl -X POST http://localhost:3000/v1/monitors -H "Content-Type: application/json" -d '{
  "name": "github api", "kind": "json_query",
  "url":  "https://api.github.com/repos/rust-lang/rust",
  "config": { "json_path": "name", "expected_value": "rust" }
}'
```

Scheduler picks new monitors up within milliseconds (reload-on-mutation via `tokio::Notify`); fallback reconcile every 30s.

Pause / resume / delete:

```bash
curl -X POST   http://localhost:3000/v1/monitors/<id>/pause
curl -X POST   http://localhost:3000/v1/monitors/<id>/resume
curl -X DELETE http://localhost:3000/v1/monitors/<id>
```

Read endpoints:

```bash
curl http://localhost:3000/v1/monitors                          # list
curl http://localhost:3000/v1/monitors/<id>                     # single
curl http://localhost:3000/v1/monitors/summary?window=86400     # per-monitor 24h rollup
curl http://localhost:3000/v1/monitors/history?per=60           # last N heartbeats per monitor
curl http://localhost:3000/v1/monitors/<id>/heartbeats?limit=100
```

---

## Real-life examples per monitor type

20 monitor types. ✅ = probe runner implemented and probing; ⏳ = the record is created but the probe returns `Down "not yet implemented"` until the runner ships.

| Kind | Real-life use case | Status |
|---|---|---|
| `http`       | Watch a website or your service's `/health` endpoint                                 | ✅ |
| `keyword`    | Catch when an upstream status page goes red (look for `"operational"` in the body)   | ✅ |
| `json_query` | Assert your API returns `{"status": "ok"}` (dotted path + expected value)            | ✅ |
| `tcp`        | Verify a port is open — Postgres, Redis, MQTT broker, any TCP service                | ✅ |
| `ping`       | Detect when your home router or VPN endpoint stops responding                        | ⏳ |
| `dns`        | Catch DNS hijack or mis-configuration on your own domain                             | ⏳ |
| `push`       | Confirm your nightly backup or cron job actually ran (the job pings *us*)            | ⏳ |
| `grpc`       | Health-check a gRPC service via the standard `grpc.health.v1` protocol               | ⏳ |
| `tls`        | Get alerted 30 days before your TLS cert expires                                     | ⏳ |
| `docker`     | Detect when your Plex / Jellyfin / Home Assistant container crashes                  | ⏳ |
| `steam`      | Watch your group's Counter-Strike, Valheim, or other Steam-based game server         | ⏳ |
| `mqtt`       | Catch silent IoT sensors that stop publishing on a given topic                       | ⏳ |
| `radius`     | Make sure your office VPN's RADIUS auth still works                                  | ⏳ |
| `kafka`      | Verify brokers are reachable before your producer starts dropping messages           | ⏳ |
| `postgres`   | Catch when your primary DB stops accepting connections (real `SELECT 1`, not TCP)    | ⏳ |
| `mysql`      | Same as Postgres but for MySQL / MariaDB                                             | ⏳ |
| `mssql`      | SQL Server availability check                                                        | ⏳ |
| `redis`      | More reliable than a raw TCP probe because it tests AUTH too                         | ⏳ |
| `mongodb`    | Detect MongoDB primary outages and replica-set failover                              | ⏳ |
| `domain`     | Reminder 60 days before your domain registration lapses (WHOIS-based)                | ⏳ |

Example payloads for the four implemented probe runners:

```bash
# http — your service's health endpoint
curl -X POST http://localhost:3000/v1/monitors -H "Content-Type: application/json" -d '{
  "name": "api health",
  "kind": "http",
  "url":  "https://api.example.com/health",
  "interval_seconds": 60,
  "accepted_statuses": [200, 204]
}'

# tcp — Postgres reachable on a private network
curl -X POST http://localhost:3000/v1/monitors -H "Content-Type: application/json" -d '{
  "name": "primary db port",
  "kind": "tcp",
  "hostname": "db.internal",
  "port": 5432,
  "interval_seconds": 30
}'

# keyword — upstream status page is still "operational"
curl -X POST http://localhost:3000/v1/monitors -H "Content-Type: application/json" -d '{
  "name": "github status",
  "kind": "keyword",
  "url":  "https://www.githubstatus.com/",
  "config": { "keyword": "All Systems Operational" }
}'

# json_query — assert {"status": "ok"} in a JSON response
curl -X POST http://localhost:3000/v1/monitors -H "Content-Type: application/json" -d '{
  "name": "api status field",
  "kind": "json_query",
  "url":  "https://api.example.com/health",
  "config": { "json_path": "status", "expected_value": "ok" }
}'
```

For the 16 stubbed kinds you can still POST and the record will be created (so you can use the wizard to capture intent now and have everything probe correctly once the runner ships). The wizard's step-1 cards show the same examples inline.

---

## Common issues

**`sqlx` macro errors at build time** — the `query!` macros need a live database to validate against. Either:
- Make sure Postgres is running before `cargo build`, OR
- Run `cargo sqlx prepare --workspace` once to generate the `.sqlx/` cache, commit it, and set `SQLX_OFFLINE=true` for CI builds.

**Port 5432 already in use** — you already have Postgres running locally. Either stop it or change the port in `compose.yaml` AND `.env`.

**Port 3000 / 5173 in use** — change `BIND_ADDR` in `.env` (backend) or the `port` in `vite.config.js` (frontend).

**UI shows "No monitors yet" after creating one** — the dashboard polls every 10s. Hard-refresh the browser if you want immediate feedback, or wait for the next tick.

---

## What to build next

Roughly in priority order:

1. **Auth.** Currently no login — anyone with network access can call the API. Add session-based auth + argon2 password hashing; the `users` / `sessions` tables already exist.
2. **Notifier crate.** Channels and templates are in the schema. Build a `rampart-notifier` crate that subscribes to status flips (the scheduler already marks `important = true` on flipping heartbeats) and fans out to Slack / Discord / Email / Webhook. Templates use the `notification_templates` table.
3. **Remaining probes.** 17 of 20 monitor kinds return `Down "not yet implemented"`. Each is a self-contained file in `backend/crates/rampart-checker/src/`. DNS and Ping are the easiest next steps; Push is just an HTTP receiver that updates a "last seen"; Domain (WHOIS) is daily-cadence and needs a separate scheduling story.
4. **Status page renderer.** Public-facing pages with custom domains. Schema is ready (`status_pages`, `status_page_groups`, `status_page_components`); the StatusPageBuilder view is the editor.
5. **Incidents + maintenance APIs.** Tables exist; no REST surface yet. Dashboard panels currently show empty states.
6. **Data importers.** Most users come from an existing monitor; a CLI that reads a SQLite or JSON export and creates the equivalent monitors + notifications + status pages makes migration realistic.

---

## License

AGPL-3.0-or-later — see [`LICENSE`](LICENSE). The strong-copyleft choice is deliberate: if a vendor builds a hosted product on top of Rampart, the AGPL's network-use clause requires they share their modifications back. If that's not a tradeoff you want, this isn't the right base for your project, and that's fine.
