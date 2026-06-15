# Rampart — full live example stack

One `docker compose up` brings up the **entire platform with data flowing
live**, so you can see every feature working before instrumenting anything of
your own. Great for evaluating, demos, and learning the dashboard.

```bash
cd examples/full-stack
docker compose up        # add -d to run detached
```

Then open **<http://localhost:3000>** and log in. The seed creates an admin on
first boot; default login is `demo@rampart.local` / `Rampart-Live-9271`.

**Use your own login** — export the creds before `up` (any password works,
the seed creates the user server-side, so the API password policy doesn't
apply):

```bash
RAMPART_ADMIN_EMAIL=me@example.com RAMPART_ADMIN_PASSWORD=hunter2hunter2 docker compose up
```

Already running and locked out? Reset/create an admin without psql:

```bash
docker compose exec rampart rampart-api reset-password me@example.com hunter2hunter2
```

Tear everything down (including the disposable database):

```bash
docker compose down -v
```

## Login says "unauthorized"?

Almost always a **stale cached image** — Docker keeps an old `latest` and your
binary predates the demo admin / `reset-password`. Force a fresh pull:

```bash
docker compose down -v        # drop old containers + db
docker compose pull           # grab the newest image
docker compose up
```

(The compose file sets `pull_policy: always`, so a plain `up` should refetch
too.) Then log in with your creds, or create one explicitly:

```bash
docker compose exec rampart rampart-api reset-password admin@example.com 'admin-pass-123'
```

## What's running

| Service | What it does |
|---|---|
| `postgres` | Rampart's database (in-memory `tmpfs` — disposable). |
| `rampart` | The platform (API + dashboard), on `:3000`. **Self-observability is on** (`RAMPART_OTLP_ENDPOINT` → itself): Rampart exports its own request traces + logs back into itself, so Traces/Logs show real live data from the running app. |
| `rampart-seed` | One-shot: migrates + seeds a baseline slice of every tier (the `[demo]` data), then exits. Includes a fixed Alertmanager ingest token. |
| `loadgen` | Signs in, creates two live monitors pointing at the probe targets, then continuously calls Rampart's own API — that real traffic is what Rampart traces + logs (no fabricated telemetry). |
| `target-healthy` | An always-200 service — its monitor stays green. |
| `target-flaky` | Returns 503 for ~20s each minute — its monitor flaps, driving uptime dips, alerts and incidents. |
| `prometheus` | Scrapes Rampart's `/metrics`, evaluates alert rules, and `remote_write`s everything back into Rampart's metrics tier (`/prom/write`). On `:9090`. |
| `alertmanager` | Routes firing alerts back into Rampart's inbound webhook, on `:9093`. |

## What to look at

- **Dashboard** — the demo monitors (with 48h of history) plus the two live
  monitors; the flaky one flips down ~20s every minute.
- **Traces / Logs** — Rampart's **own** request spans + internal logs, live, as
  the load generator drives real API traffic (service name `rampart`).
- **RUM** — real Core Web Vitals from the dashboard itself (the RUM snippet is
  injected via `RAMPART_SELF_RUM`; app `rampart-dashboard`). Click around to
  generate beacons.
- **Errors** — the seeded `[demo]` baseline plus any **real** browser JS errors
  the snippet catches while you use the UI.
- **Detection** (`#/detection`) — a seeded SIEM rule with a raised finding.
- **Alert rules / Escalations** — a seeded telemetry rule.
- **Status page → incidents** — once the flaky monitor stays down past the
  Prometheus `for:` window, Prometheus → Alertmanager → Rampart's inbound
  webhook opens an incident. (Prometheus UI: `:9090`, Alertmanager: `:9093`.)

## The alert → incident path

`prometheus/alerts.yml` fires `RampartMonitorDown` when any monitor is down.
Alertmanager (`alertmanager/alertmanager.yml`) posts it to

```
POST http://rampart:3000/v1/public/ingest/alertmanager/<token>
```

The `<token>` is a **fixed** value the seeder creates
(`ing_demo_alertmanager_000000000000000000`), so the stack is turnkey with no
manual token minting. In a real deployment you mint a per-Status-page token and
paste its URL into Alertmanager — see [docs/INGEST.md](../../docs/INGEST.md).

## Notes

- Ingest auth is left open here (no telemetry token) so the load generator can
  post without credentials — fine for a local demo, not for production.
- Uses the published image `ghcr.io/pen-pal/rampart:latest`. To exercise local
  changes instead, set `RAMPART_IMAGE=` and uncomment the `build:` lines on the
  `rampart` and `rampart-seed` services in `docker-compose.yml`.
- This is a **demo**: the database is `tmpfs`, so everything resets on
  `down -v`.
