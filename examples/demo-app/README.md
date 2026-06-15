# Rampart demo app

A **real, instrumented sample application** — Node/Express backend + browser
frontend, with its own Postgres and Redis — wired to a Rampart instance so you
can watch *every* tier fill with genuine data, not fixtures.

```bash
cd examples/demo-app
docker compose up --build
```

- **App**: <http://localhost:8088> — drives traffic on its own (and buttons to
  poke it manually).
- **Rampart**: <http://localhost:3000> — log in `demo@rampart.local` /
  `Rampart-Live-9271` (override with `RAMPART_ADMIN_EMAIL` / `_PASSWORD`).

Tear down: `docker compose down -v`.

## What you'll see in Rampart

| Tier | Source |
|---|---|
| **Traces** | Auto-instrumented spans for every request — Express → Postgres → Redis, including a multi-step `/api/checkout`. Service `demo-backend`. |
| **Logs** | Structured backend logs via OTLP, including the SIEM auth lines. |
| **Profiling** | The backend takes a periodic V8 CPU profile, folds it, and pushes it to `/profiles/v1/folded` (service `demo-backend`, type `cpu`). |
| **RUM** | The frontend loads Rampart's RUM snippet (`app=demo-frontend`) → real Core Web Vitals. |
| **Errors** | Backend `/api/boom` 500s + uncaught browser errors the RUM snippet forwards. |
| **SIEM / Detection** | The backend logs repeated `failed login …` lines; `demo-setup` creates a detection rule (`service=demo-backend`, `failed login`, ≥3 / 10 min) that raises a finding. |
| **Uptime** | `demo-setup` adds an HTTP monitor on the backend's `/api/health`. |

## How it's wired

- **Backend** (`backend/`): `node -r ./otel.js server.js`. `otel.js` starts the
  OpenTelemetry Node SDK (auto-instrumentations for http/express/pg/ioredis →
  OTLP traces), an OTLP log pipeline, and the CPU-profile pusher. All point at
  `RAMPART_OTLP` / `RAMPART_PROFILES` (the Rampart container).
- **Frontend** (`frontend/`): static page behind nginx (which proxies `/api` →
  backend). It loads `http://localhost:3000/rum/snippet.js` so the browser
  reports web vitals + JS errors straight to Rampart.
- **Stores**: `demo-db` (Postgres) + `demo-redis` are the app's own data — every
  query shows up as a child span.

## Notes

- This is a reference example; `docker compose up --build` builds the backend
  image (runs `npm install`). It's separate from `examples/full-stack` (which
  dogfoods Rampart's *own* telemetry); this one is a *distinct app* you observe.
- RUM snippet URL is hard-coded to `http://localhost:3000` (host-published
  Rampart). If you run Rampart elsewhere, edit `frontend/index.html`.
- Metrics: not pushed here (Rampart ingests Prometheus `remote_write` /
  text-push — see `examples/full-stack`). Everything else is live.
