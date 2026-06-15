# Demo data

`rampart-api seed-demo` fills a fresh instance with one representative slice of
**every tier**, so the dashboard shows a living system before you've wired up
any real telemetry. Great for evaluating, screenshots, or learning the UI.

## Run it

```bash
# Docker compose (service name `rampart`):
docker compose exec rampart rampart-api seed-demo

# Single binary:
DATABASE_URL=postgres://rampart:rampart@localhost:5432/rampart \
  ./rampart-api seed-demo
```

It runs migrations first (same as a normal boot), seeds, prints a summary, and
exits — it does **not** start the server.

## What it creates

| Tier | Seeded |
|---|---|
| Monitors | A `[demo]` folder with 4 monitors (HTTP / keyword / TCP / Redis), each with 48h of hourly heartbeats — the Cache takes a short outage so the uptime strip shows a dip. One carries an SLO target. |
| Errors | A `[demo] web` project with 2 grouped issues; one recurs across 3 releases + 3 users (so issue stats show users-affected / by-release) and carries a breadcrumb trail. |
| Traces | A 3-span trace across `[demo] api` → `[demo] payments`, with an errored leaf span. |
| Logs | 7 log lines across services + levels, including repeated `failed login` events. |
| RUM | 3 web-vitals beacons (one with a poor LCP/CLS). |
| Metrics | A 2-instance `demo_requests_per_sec` / `demo_p95_latency_ms` series. |
| Alerting | A telemetry alert rule (trace error-rate) and a notification channel. |
| Detection | A SIEM rule (`failed login`, severity high) that evaluates immediately and **raises a finding** from the seeded auth logs. |

## Live example stack

`seed-demo` gives a static baseline. For a **live** demo where data keeps
flowing — traces/logs/RUM/errors streaming in, a monitor genuinely flapping,
Prometheus → Alertmanager opening incidents — use the full-stack compose example
at `examples/full-stack/`:

```bash
cd examples/full-stack
docker compose up
```

It runs Rampart + Postgres + this seeder + a load generator + healthy/flaky
probe targets + Prometheus + Alertmanager, all wired together. See
`examples/full-stack/README.md` for the tour.

## Idempotency + cleanup

The seeder keys off the `[demo] Sample services` folder: if it already exists,
re-running does nothing. Everything it creates is prefixed `[demo]`, so to
remove it later delete that monitor folder, the `[demo] web` error project, the
`[demo]` detection + alert rules and notification channel from the UI. (The
trace/log/RUM/metric rows age out on their own via retention.)
