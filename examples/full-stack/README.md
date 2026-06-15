# Rampart — full live example stack

One `docker compose up` brings up the **entire platform with data flowing
live**, so you can see every feature working before instrumenting anything of
your own. Great for evaluating, demos, and learning the dashboard.

```bash
cd examples/full-stack
docker compose up        # add -d to run detached
```

Then open **<http://localhost:3000>**. The load generator registers an admin
(`demo@rampart.local` / `demo-password-123`) on first boot — log in with that,
or take the first-run signup yourself before it does.

Tear everything down (including the disposable database):

```bash
docker compose down -v
```

## What's running

| Service | What it does |
|---|---|
| `postgres` | Rampart's database (in-memory `tmpfs` — disposable). |
| `rampart` | The platform (API + dashboard), on `:3000`. |
| `rampart-seed` | One-shot: migrates + seeds a baseline slice of every tier (the `[demo]` data), then exits. Includes a fixed Alertmanager ingest token. |
| `loadgen` | Continuously emits **OTLP traces + logs**, **RUM** web-vitals and **errors**, and creates two live monitors pointing at the probe targets. |
| `target-healthy` | An always-200 service — its monitor stays green. |
| `target-flaky` | Returns 503 for ~20s each minute — its monitor flaps, driving uptime dips, alerts and incidents. |
| `prometheus` | Scrapes Rampart's `/metrics` and evaluates alert rules, on `:9090`. |
| `alertmanager` | Routes firing alerts back into Rampart's inbound webhook, on `:9093`. |

## What to look at

- **Dashboard** — the demo monitors (with 48h of history) plus the two live
  monitors; the flaky one flips down ~20s every minute.
- **Traces / Logs / RUM** — moving as the load generator emits; the RUM LCP
  chart wanders and occasionally breaches.
- **Errors** — the seeded `[demo] web` issues plus a live `live-app` stream.
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
