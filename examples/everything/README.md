# Rampart · the "everything" demo

The most exhaustive Rampart demo: **one `docker compose up`** brings up a live
stack that exercises *every* Rampart feature with **genuinely real data** — real
OTLP traces/logs/metrics from a real instrumented app, real exceptions via a real
Sentry SDK, real RUM from a real browser page, real push heartbeats from a real
worker, real Prometheus scrape + `remote_write`, and real probe targets Rampart's
own scheduler hits (so up/down/latency/cert are genuine). The API/CLI is used
**only for config** (creating monitors, channels, rules, status pages, orgs,
keys, agents, reports) — never to fabricate telemetry/uptime/errors.

```bash
cd examples/everything
cp .env.example .env          # optional — defaults work out of the box
docker compose up             # default profile (lean targets)
```

| What | URL | Notes |
|---|---|---|
| **Rampart UI** | <http://localhost:3000> | log in `demo@rampart.local` / `Rampart-Live-9271` |
| **Demo app** | <http://localhost:8088> | drives traffic on its own; buttons to poke it |
| **Webhook sink** | <http://localhost:8099> | every *real* notification delivery shows here (auto-refreshes) |
| **Mailpit** | <http://localhost:8025> | real emails (down alerts, SLO breach, scheduled report, subscriber) |
| **Prometheus** | <http://localhost:9090> | scrapes rampart + demo-app, `remote_write`s into Rampart |
| **Alertmanager** | <http://localhost:9093> | posts alerts → Rampart's ingest webhook → incidents |
| **Dex (SSO)** | <http://localhost:5556/dex> | only with `--profile oidc` |

Profiles:

```bash
docker compose --profile heavy up   # + exotic probe targets (mysql, mssql, mongo,
                                     #   elasticsearch, vault, etcd, redpanda,
                                     #   cassandra, ldap, rabbitmq, nats, memcached,
                                     #   mqtt, radius, ntp, snmp, ssh/ftp/imap/pop3, grpc)
docker compose --profile oidc  up    # + Dex SSO (set the RAMPART_OIDC_* vars in .env)
```

Tear down: `docker compose down -v`.

## What to watch (give it ~3–5 minutes of uptime)

- **Monitors** — 50+ monitors covering **all 42 MonitorKinds**. A *flapping*
  HTTP monitor (`edge · flapping ready probe`) and a 503-every-minute target
  (`edge · flaky target`) genuinely cycle Down/Up → uptime history, an **open
  episode**, escalation + on-call paging, SLO burn. A `check_cert` HTTP monitor
  against a ~10-day cert shows **Warn**; a `tls` monitor against an expired cert
  shows **Down**. Heavy/exotic kinds sit in the *Heavy (profile)* folder and
  flip Up once you run `--profile heavy`.
- **Traces / Logs** — real OTLP from the instrumented `demo-backend`
  (Express→pg→redis spans, a multi-step `/api/checkout`) *and* Rampart's own
  self-telemetry. Logs include the SIEM `failed login …` lines.
- **Metrics** — Prometheus scrapes rampart + demo-app and `remote_write`s into
  Rampart; a `metrics-pusher` cron also pushes `demo_queue_depth` via the
  authenticated text-push API → breaches the **metric rule**.
- **Profiling** — the app pushes folded CPU profiles continuously; `provision`
  also curls a captured **pprof** (gzipped protobuf) and an **OTLP-profiles**
  protobuf once, so all three profiling formats genuinely hit.
- **RUM / Errors** — the browser page loads Rampart's RUM snippet (web vitals +
  uncaught JS errors); backend 500s are captured by `@sentry/node` into a real
  **error project** (DSN minted by `provision`).
- **Detection (SIEM)** — a rule keys on the backend's repeated `failed login`
  logs and **raises findings**.
- **Notifications** — **all ~128 channel kinds** exist as config (visible in the
  UI). Real deliveries fire for the sink-compatible kinds (webhook, mattermost,
  rocketchat, gotify, ntfy, matrix, apprise, home_assistant, alerta, … + email →
  Mailpit) and land in the **webhook-sink** / **Mailpit**. Vendor kinds
  (slack/discord/pagerduty/twilio/…) are created + labelled *"needs real creds"*
  and are **not** test-fired.
- **Incidents** — `provision` posts a firing+resolved alert through the real
  Alertmanager ingest webhook (opens then closes an incident); Alertmanager
  keeps doing so live as monitors flap.
- **Multi-tenancy (RLS enforced)** — two orgs (`Default` + a fully-populated
  `Demo Team`) each with their OWN monitors (`demo-team · …`) and telemetry
  (per-org ingest key → logs, plus org-scoped metrics). Isolation is enforced at
  the Postgres layer via row-level security (`RAMPART_RLS=1`) — defense-in-depth
  on top of the app-level scoping — so a switched-in tenant cannot see another
  org's data. Also: a member (re-roled editor→readonly), a real org switch, and a
  non-member 404 probe.
- **Push monitor** — a `push-cron` worker sends real `run`/`complete`
  heartbeats, and `/fail`s every 6th cycle → a genuine Down flip → paging.
- **Remote agent** — a from-source `rampart-agent` probes a private-only target
  (`agent-target`, reachable only inside the network) and reports heartbeats.

Run the assertions:

```bash
bash verify.sh        # needs jq + curl; asserts every tier is non-empty
```

## How each feature is exercised LIVE (coverage map)

| Feature | Real mechanism |
|---|---|
| Monitors — all 42 kinds | `provision` POSTs `/v1/monitors` against real in-network targets (lean default + `--profile heavy`); the scheduler probes them for real |
| Flapping / outage / episode | `target-flaky` returns 503 for 25s/min; `edge · flapping ready probe` hits the toggleable `/api/ready` |
| Cert expiry Warn / TLS Down | `tls-target` generates a ~10-day cert (`check_cert` Warn) and a faketime-2020 expired cert (`tls` Down) at startup |
| Push monitor | `push-cron` POSTs `/push/<token>/{run,complete,fail}` (token captured by `provision`) |
| Synthetic | a 2-step login→whoami flow with var extraction against the demo backend |
| Traces / Logs | `@opentelemetry/*` auto-instrumentation → `/otlp/v1/{traces,logs}`; plus Rampart self-telemetry (`RAMPART_OTLP_ENDPOINT`) |
| Metrics | Prometheus `remote_write` → `/prom/write`; `metrics-pusher` → authenticated `/v1/metrics/ingest` |
| Profiling (folded) | the app folds V8 CPU profiles → `/profiles/v1/folded` every 30s |
| Profiling (pprof + OTLP) | `provision` curls captured `fixtures/profile.pb.gz` → `/profiles/v1/pprof` and `fixtures/otlp-profiles.pb` → `/otlp/v1development/profiles` |
| RUM | the browser page loads `/rum/snippet.js`; Rampart's own UI also injects it (`RAMPART_SELF_RUM`) |
| Errors | `@sentry/node` → DSN (`http://<public_key>@rampart:3000/<project_id>`) minted by `provision` |
| Detection (SIEM) | rule on `service=demo-backend` + `failed login` ≥3/10m, fed by real auth-fail logs |
| Telemetry rules | error_rate / trace_latency / log_volume rules over the real telemetry |
| Metric rule | `demo_queue_depth > 40`, breached by the pushed series |
| Notifications (real) | sink-compatible channels → `webhook-sink` + Mailpit (real SMTP) |
| Notifications (config only) | vendor-hardcoded kinds created + labelled "needs real creds" |
| Escalation + on-call | policy bound to the flapping monitor; 5-min rotation schedule |
| SLO | tight 99.99% on the flapping monitor — error budget burns for real |
| Maintenance + silence | a window covering now + a recurring weekly one; a silence created then lifted |
| Status pages | public page (sections, custom css/logo, subscriber, incident + updates) + a private password-protected page |
| Incidents (ingest) | firing+resolved alerts through the real `/v1/public/ingest/alertmanager/<token>` |
| Ingest tokens | minted per vendor (alertmanager, grafana, datadog, pagerduty, opsgenie, generic + mapping) |
| Multi-tenancy (RLS) | 2 orgs, each with isolated monitors + telemetry (per-org ingest key); isolation enforced at the Postgres layer via row-level security (`RAMPART_RLS=1`); + member re-role, real switch, non-member 404 |
| API keys | read / write / admin scopes (the write key drives the metrics-pusher) |
| Proxy | created + a monitor routed through it |
| Remote agent | built from source, probes a private-only target |
| Scheduled report | created → really emails a weekly uptime report to Mailpit |
| Deploy markers, presets, templates | created via API |
| Notification (Liquid) template | created + attached to the webhook channel |
| CSV import / export + bulk ops | round-trip import + a pause/resume bulk action |

## Features that need external creds / internet (flagged)

- **Vendor-hardcoded channels** (slack, discord, telegram, pagerduty, twilio,
  opsgenie, datadog, sentry, aws_sns, …): created as config + labelled *"needs
  real creds"*. They are **not** `/test`-ed (that would hit the real vendor).
  Paste real creds in the UI to make any of them deliver.
- **Egress monitors** (`domain` WHOIS, `rdap`, `doh`, `steam`): need outbound
  internet; named `egress · … (needs internet)`. They show Down in an air-gapped
  network — that's a real result, not a fixture.
- **`browser` monitor**: needs an external headless renderer service
  (`renderer_url`); named `egress · browser (needs renderer)`.
- **OIDC**: only active under `--profile oidc`; set the `RAMPART_OIDC_*` vars in
  `.env` (defaults target the bundled Dex; log in as `sso-user@rampart.local` /
  `password`).
- **`docker` monitor**: probes a container via `/var/run/docker.sock` (mounted
  read-only on the rampart container). If your host's daemon socket isn't group-
  readable by the container user, this monitor stays Down (still a real result).

## Known caveats

- **`RAMPART_SECRET_KEY` is unset by default.** On the published image
  (v0.137.x) the live monitor-flip notification fan-out reads the channel config
  *without decrypting it* (`rampart-db` `routing::resolve_channels_for_monitor`
  maps `config` raw), so with a key set the flip-path deliveries fail
  `missing field url` — while `/test`, digest and scheduled paths decrypt fine.
  To keep the demo's headline *real deliveries* working out of the box, the key
  is left unset (channel secrets stored plaintext). Set
  `RAMPART_SECRET_KEY=$(openssl rand -hex 32)` in `.env` to demo
  encryption-at-rest (flip-path deliveries then degrade until that upstream bug
  is fixed).
- **Two newest channel kinds** (`sms46elks`, `whatsapp360`) only exist in the DB
  `channel_kind` enum on recent images; on an older published image their create
  is a harmless 500 (best-effort), so you'll see ~125 of the ~128 kinds.
- **`heavy` profile is resource-hungry** (Elasticsearch, MSSQL, Cassandra, …).
  Give Docker a few GB of RAM, or bring up only the heavy services you want.
- The remote-agent service **always builds from source** (`cargo build -p
  rampart-agent`) — the published image omits the agent binary. First `up` will
  spend a few minutes compiling it.

## Building Rampart from this repo instead of the published image

```bash
# in .env: RAMPART_IMAGE=    (empty)
# then uncomment the `build:` line on the rampart service in docker-compose.yml
docker compose build rampart
docker compose up
```

## Layout

```
everything/
├── docker-compose.yml         # the whole stack (default + heavy + oidc profiles)
├── .env.example               # creds / image / secret-key / oidc knobs
├── verify.sh                  # asserts every tier is non-empty
├── provision/                 # one-shot config provisioner (idempotent)
│   ├── Dockerfile             #   alpine + bash/curl/jq
│   ├── provision.sh           #   orchestrator (auth + run sections)
│   └── sections/              #   10-secrets … 99-finalize
├── demo-app/                  # the REAL instrumented app
│   ├── backend/               #   Node: OTLP traces/logs, folded profiles, Sentry, /metrics
│   └── frontend/              #   browser page w/ the RUM snippet + autodrive
├── webhook-sink/              # browser-viewable echo for real channel deliveries
├── metrics-pusher/            # cron: real prom-text push (write api-key)
├── push-cron/                 # cron: real push-monitor heartbeats (+ /fail flips)
├── prometheus/                # scrape + remote_write + alert rules
├── alertmanager/              # posts to the real ingest webhook (token templated in)
├── targets/                   # flaky HTTP + TLS (warn/expired cert) targets
├── agent/                     # remote agent, built from source
├── dex/                       # OIDC provider (oidc profile)
└── fixtures/                  # captured pprof + OTLP-profiles protobufs (+ generator)
```
