<div align="center">

<img src="docs/assets/logo.svg" alt="Rampart Logo" width="100" />

# Rampart

### Self-hosted uptime monitoring **and** observability you can actually trust.

**One Rust binary. One Postgres. 38 probe kinds. 130 notification channels.**<br/>
Uptime + status pages, **error tracking, distributed traces, logs, and RUM** — one binary, no SaaS.<br/>
Tier alerting • On-call rotations • Multi-step synthetics • **SSO (OIDC)** • **HA (leader election)** • encrypted secrets • SSRF-guarded probes • tamper-evident audit • 2FA.

[![License](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](LICENSE)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-dea584.svg?logo=rust)](https://www.rust-lang.org/)
[![Postgres](https://img.shields.io/badge/database-Postgres%2014%2B-336791.svg?logo=postgresql)](https://www.postgresql.org/)
[![Probes](https://img.shields.io/badge/probes-38-brightgreen.svg)](#-38-probe-kinds)
[![Channels](https://img.shields.io/badge/channels-130-brightgreen.svg)](#-130-notification-channels)
[![Bundle](https://img.shields.io/badge/binary-~10%20MB-informational.svg)](#-why-rampart)

<br/>

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/dashboard-dark.png">
  <img src="docs/assets/dashboard.png" alt="Rampart dashboard — folder tree sidebar, mixed-state health banner, live response-time chart" width="100%"/>
</picture>

<sub>✨ Light & dark themes — switches automatically based on your system preferences.</sub>

</div>

---

## 🚀 Quick Start

Get up and running in under 60 seconds. No SaaS accounts, no agents, no extra services.

```bash
git clone https://github.com/pen-pal/rampart.git
cd rampart
docker compose up -d
```

👉 **Open [http://localhost:3000](http://localhost:3000)** — Your first visit automatically creates the admin account. Migrations run on boot. That's it.

> 🎬 **Want the whole dashboard populated to look around first?** Run `docker compose exec rampart rampart-api seed-demo` — it fills every tier (monitors, errors, traces, logs, RUM, a detection finding, an alert rule, an SLO) with tagged `[demo]` data. Idempotent. See [`docs/DEMO.md`](docs/DEMO.md).
>
> 🧪 **Or run the whole platform with data flowing live** (Rampart + Postgres + a load generator + flaky probe target + Prometheus + Alertmanager, one command): [`examples/full-stack/`](examples/full-stack/). `cd examples/full-stack && docker compose up`.
>
> 🧩 **Or a real instrumented sample app** (Node + browser, Postgres + Redis) that exercises every tier — traces, logs, profiling, RUM, errors, SIEM: [`examples/demo-app/`](examples/demo-app/). `cd examples/demo-app && docker compose up --build`.

> 📖 **Want a step-by-step tour?** [`docs/WALKTHROUGH.md`](docs/WALKTHROUGH.md) walks the full first-run journey — admin setup, probe wizard, first heartbeats, notification channels, status pages — with a labelled screenshot for every step. New here? Start there.

---

## 🛡️ Why Rampart?

We built Rampart because we were tired of choosing between bloated SaaS tools and fragile, half-baked self-hosted dashboards.

| Feature | The Rampart Way |
| :--- | :--- |
| 📦 **One Binary** | Frontend embedded via `rust-embed`. ~10 MB stripped release binary. No Node runtime on the host. |
| 🐘 **Postgres-backed** | The DB you already operate. No SQLite weirdness, no proprietary stores. |
| 🦀 **Pure-Rust Crypto** | Web Push (RFC 8291) hand-rolled with `p256` + `aes-gcm`. No `aws-lc-rs` / `openssl` dragged in. |
| ⚡ **Live, Not Polled** | Server-Sent Events stream heartbeats to the dashboard; the UI never refreshes from cache. |
| 🕵️ **Zero Telemetry** | Nothing phones home. Self-hosted means *actually* self-hosted. |
| ⚖️ **AGPL-3.0** | Modifications you serve over the network must be shared back. Source-available, not source-thrown-over-the-fence. |

### How it compares

- **vs. SaaS (Datadog / Pingdom / Site24x7)** — Lives on your hardware, no per-monitor pricing, no log-volume bills, no data leaving your perimeter.
- **vs. other self-hosted dashboards** — Broader probe catalog (DBs, banner protocols, Kafka, RADIUS, NTP), proper tag routing with folder ancestor inheritance, real audit log, Postgres instead of SQLite, a single Rust binary instead of a Node runtime + headless Chromium.
- **vs. roll-your-own Prometheus blackbox** — Out of the box: status pages, incident posting, maintenance windows, dependency-aware alerting, and 130 outbound channels.
- **vs. a separate APM/error stack (Datadog / Sentry / Grafana LGTM)** — Error tracking, traces, logs, RUM, and continuous profiling (flamegraphs) live in the *same* binary as your uptime checks, speak OpenTelemetry + Sentry + pprof wire formats (no proprietary agent), and alert through the same channels — instead of standing up and paying for a second platform.

---

## ✨ Features at a Glance

### 🔍 38 Probe Kinds
Every probe supports per-monitor intervals, timeouts, retries, and re-alerts.

| Category | Supported Kinds |
| :--- | :--- |
| 🌐 **HTTP Family** | HTTP, Keyword (substring), JSON query (JSONPath + expected value) |
| 📡 **Network** | TCP, ICMP ping, DNS (A/AAAA/CNAME/MX/TXT/NS/SRV/CAA/SOA), TLS cert days-left, DNS-over-HTTPS (RFC 8484), Domain expiry (WHOIS), RDAP (RFC 7480/9082), NTP (SNTPv4) |
| 🗄️ **Databases** | Postgres, MySQL, MSSQL, Redis, MongoDB, Memcached, Cassandra/ScyllaDB |
| 📢 **Messaging / RPC** | gRPC `health.v1`, MQTT, Kafka (ApiVersions handshake), NATS, LDAP, AMQP, SNMP, mDNS, SSDP/UPnP, RADIUS |
| 🐳 **Containers** | Docker daemon |
| 🏷️ **Banner Protocols** | SSH, SMTP, IMAP, FTP, POP3 (greeting prefix check, `expect` overridable) |
| 🎯 **Specialty** | Push (anything POSTing to `/push/:token`), headless-browser keyword, Steam (A2S) |

*HTTP probes include methods, accepted statuses, custom headers/body, follow-redirects, ignore-TLS, and proxy support. Soft-fail "warn" statuses are supported where applicable (e.g., NTP stratum 0).*

### 🔔 130 Notification Channels
Liquid-templated subject + body, per-channel cooldown, HMAC-signed Generic Webhooks, and tag-based auto-routing.

<details>
<summary><strong>Click to expand all 130 channels — grouped by category</strong></summary>

<br/>

| Category | Channels |
| :--- | :--- |
| 💬 **Team chat** | Slack · Discord · Microsoft Teams · Mattermost · Rocket.Chat · Matrix · Google Chat · Zulip · Pumble · Lark · Webex · Flock · ZohoCliq · Bitrix24 · Stackfield · MAX · Kook · OneChat · OneBot |
| 📨 **Telegram & friends** | Telegram · Signal · WhatsApp via WAHA · Threema · Mastodon · Nostr · Pushy · Pushbullet · Pushcut · Bale |
| 🇨🇳 **APAC chat / push** | WeCom · DingTalk · Feishu · Line · Bark · ServerChan · PushPlus · PushDeer · SpugPush · WPush · YZJ · VK |
| 📱 **Mobile push** | Pushover · Gotify · ntfy · Notifery · Onesender · Gorush · Fluxer · Splash · Evolution |
| 📧 **Email (transactional)** | Generic SMTP · SendGrid · Resend · Brevo · Mailgun · Mailjet · Postmark · Mandrill · SparkPost |
| 📞 **SMS providers** | Twilio · Aliyun SMS · ClickSend · 46elks · CallMeBot · Telnyx · MessageBird · Plivo · Vonage · Bandwidth · SMSEagle · Octopush · SerwerSMS · SMSPlanet · SMSC.ru · Cellsynt · seven.io · GtxMessaging · PromoSMS · SMSPartner · SMS.ir · FreeMobile · SMSGlobal · SmsManager · Teltonika · Whapi · 360messenger |
| 🚨 **Incident & on-call** | PagerDuty · Opsgenie · PagerTree · Squadcast · GoAlert · Alerta · AlertNow · SIGNL4 · Heii On-Call · Splunk On-Call · Grafana OnCall · AlertOps · Spike.sh · Zenduty · RingCentral · iLert · FlashDuty · Halo PSA · Jira Service Management |
| 🎫 **Issue trackers / PM** | Linear · ClickUp · Trello · GitHub Issues · GitLab Issues · Asana · Notion |
| 📊 **Observability** | Sentry · Rollbar · Honeybadger · Healthchecks.io · BetterStack · Statuspage.io · Datadog Events · New Relic Events |
| ☁️ **Cloud event bus** | AWS SNS · Azure Service Bus · GCP Pub/Sub |
| 🏠 **Smart home / IoT** | Home Assistant |
| 🧰 **Programmable / catch-all** | Apprise gateway · Generic Webhook (HMAC-signed) · Web Push (browser, RFC 8291) · Google Sheets (Apps Script) |

</details>

### 📊 Observability Platform

More than uptime — Rampart bundles the four observability tiers a small team
actually needs, each wired into the same alert/notify/escalation spine. A
self-hostable alternative to Datadog / Sentry / Site24x7 / ScoutAPM, in one binary.

| Tier | What it does | Wire-compatible with |
| :--- | :--- | :--- |
| 🐞 **Error tracking** | DSN-keyed event ingest, group-by-fingerprint into issues, new/regressed alerts, stack traces. | **Sentry SDKs** (point the DSN at Rampart — no Rampart SDK) |
| 🧵 **Traces / APM** | Span ingest, per-trace waterfall, service dependency map, and a per-operation **APM rollup** (calls, error rate, p50/p95/p99 latency). | **OpenTelemetry** OTLP/HTTP (JSON + protobuf, gzip) |
| 📃 **Logs** | Severity + service filtering, full-text body search (`tsvector`), **live tail**, per-level volume bar, and trace↔log correlation. | **OpenTelemetry** OTLP logs |
| 👁️ **RUM** | Browser snippet → Core Web Vitals (p75 LCP/INP/CLS), per-page vitals, and **JS error capture** into the error tier. | drop-in `<script>`, no build step |
| 🔥 **Profiling** | Continuous profiling → **flamegraph** (icicle, click-to-zoom) + top-functions table, merged over a service/type window. | **pprof**, **OTLP profiles**, and folded text |

Cross-tier by design: a trace links to the logs emitted under its `trace_id`,
an error issue jumps to its trace, and a browser exception becomes an error issue.

| Alerting & response | What it does |
| :--- | :--- |
| 🔔 **Tier alert rules** | Threshold rules over the tiers — error-rate, trace p95 latency, trace error-rate, log volume — paging the same channels. |
| 📈 **Metric rules** | Prometheus-text metric ingest + threshold alerts on any series. |
| 🎯 **SLOs + error budgets** | Named objectives over monitor uptime or a metric ratio; rolling error budget with exhaustion + fast-burn (1h burn-rate) paging into channels/escalation. |
| 🪜 **Escalations** | Ordered notification ladders with acknowledge + episode lifecycle. |
| 📟 **On-call rotations** | Rotating channel schedules feeding ladder steps. |
| 🔐 **Ingest auth** | Optional shared token gating the OTLP + RUM endpoints; gzip/deflate decode for stock collectors. |

> Telemetry ingest is OpenTelemetry- and Sentry-wire-compatible, so you point
> existing SDKs/collectors at Rampart instead of adopting a proprietary agent.

### 🛠️ Beyond the Probe

| Capability | What it does |
| :--- | :--- |
| 🌍 **Status Pages** | Public, no-login pages at `/#/s/:slug`. Worst-status-wins rollup, 90-day uptime per component, incidents with running updates, email subscribers, dark/light theme. |
| 📁 **Folders & Routing** | Nested folders group monitors. Tag a folder/monitor/channel; channels auto-route to monitors sharing a tag. Inheritance propagates down. Per-monitor exclusions supported. |
| 🔗 **Dependencies** | A down parent silences dependents so one root cause doesn't trigger a paging storm. Cycle-guarded. |
| 🛠️ **Maintenance** | Time-windowed suppression of probes + notifications. One-shot / daily / weekly recurrence. |
| 🏷️ **Tags** | Colored chips. Dashboard filter (AND semantics, persistent). Inline editor on monitor detail. |
| 📦 **Bulk & Clone** | Multi-select dashboard actions (pause/resume/delete/move). One-click monitor clone. Per-monitor heartbeat CSV export. |
| ⚡ **Live UI** | Dark / light / system theme. Server-Sent Events live heartbeat stream. Responsive mobile layout. |
| 🔐 **Auth & Security** | Session cookie + 2FA (TOTP + 10 recovery codes), **SSO via OIDC** (Google/Okta/Keycloak/Authentik…), API keys (`rmp_…`), multi-user RBAC, and a **tamper-evident audit log** (HMAC hash chain) that now records **auth events** (login, failed login, 2FA failure) with a one-click security filter + in-UI **integrity verification**. |
| 🛡️ **Hardening** | **SSRF guard** on every outbound probe (blocks cloud-metadata/internal), notification + SMTP **secrets encrypted at rest** (AES-256-GCM), rate-limited + optionally-authenticated ingest. |
| 🧬 **High availability** | Postgres advisory-lock **leader election** — run multiple replicas; one owns the scheduler, the rest serve the API, automatic failover. No duplicate probes or alerts. |
| 🗑️ **Retention** | Hourly prune loop for heartbeats + audit log. Windows configurable in the admin UI. |
| 📝 **Templates** | Liquid templating for notification subject + body — filters, conditionals, loops. Clone existing templates. |
| 🌐 **Proxies** | HTTP/SOCKS proxy registry; HTTP-family monitors route through their assigned proxy. |
| 📜 **Cert Tracking** | Auto-inspect leaf cert on HTTPS monitors every hour. Days-left badge on detail page. |

---

## 🏗️ Architecture

```text
rampart/
├── backend/                              # Rust workspace (axum + sqlx)
│   └── crates/
│       ├── rampart-core                  # pure types, no I/O
│       ├── rampart-db                    # sqlx repository layer
│       ├── rampart-checker               # probe runners (38 kinds)
│       ├── rampart-scheduler             # per-monitor tokio tasks + batched writer
│       ├── rampart-notifier              # channel fan-out (130 adapters)
│       └── rampart-api                   # axum HTTP server (embeds React)
├── frontend/                             # Vite + React SPA
├── docs/                                 # architecture, setup, security debt
├── Dockerfile                            # multi-stage: frontend → rust → debian-slim
├── compose.yaml                          # production stack (postgres + rampart)
└── backend/compose.yaml                  # dev stack (postgres only)
```

> **Out of scope (deliberate):** Multi-region distributed probing, workspace multi-tenancy (Rampart is single-tenant by design), and an inline **tunnel / proxy data plane** (private-network reach is the [probe agent](docs/AGENTS.md)'s job — outbound-only, no inbound holes; see [TUNNELING.md](docs/design/TUNNELING.md)). The observability tiers (error tracking, traces, logs, RUM, profiling) are scoped for small-team self-hosting, not hyperscale APM. See [CONTRIBUTING.md](CONTRIBUTING.md#scope-read-this-first) for the philosophy.

---

## 💻 Installation

### 🐳 Docker (Recommended)

```bash
git clone https://github.com/pen-pal/rampart.git
cd rampart
docker compose up -d           # builds image + starts postgres
```
👉 Open [http://localhost:3000](http://localhost:3000)

*For a step-by-step walkthrough including TLS, reverse proxy, and backups, see [**docs/SETUP.md**](docs/SETUP.md).*

### 📦 Single Binary (No Docker for the app)

```bash
# 1. Start Postgres
cd backend && docker compose up -d postgres

# 2. Build frontend & backend
cd ..
( cd frontend && npm ci && npm run build )
( cd backend && cargo build --release -p rampart-api )

# 3. Run
DATABASE_URL=postgres://rampart:rampart@localhost:5432/rampart \
  ./backend/target/release/rampart-api
```

### ☸️ Kubernetes (Helm)

Production-grade chart published to GHCR (OCI) — HPA, PDB, topology spread,
Ingress (+ cert-manager), Istio service mesh, NetworkPolicy, Prometheus
ServiceMonitor, non-root/read-only-rootfs. Bring your own Postgres.

```bash
kubectl create secret generic rampart-db \
  --from-literal=DATABASE_URL='postgres://user:pass@pg-host:5432/rampart'

helm install rampart oci://ghcr.io/pen-pal/charts/rampart \
  --version 0.2.0 \
  --set externalDatabase.existingSecret=rampart-db
```

Full guide: [**docs/KUBERNETES.md**](docs/KUBERNETES.md) · chart: [`charts/rampart`](charts/rampart).

### 🛠️ Dev Mode (Hot Reload)

```bash
# Terminal 1 — Postgres
cd backend && docker compose up -d postgres

# Terminal 2 — backend (auto-rebuilds)
cd backend && cargo run -p rampart-api

# Terminal 3 — frontend with HMR
cd frontend && npm run dev          # localhost:5173 proxies API to :3000
```

---

## ⚙️ Configuration

All configuration is done via environment variables. Defaults are in `backend/.env.example`.

| Variable | Default | Description |
| :--- | :--- | :--- |
| `DATABASE_URL` | `postgres://...` | Postgres connection string. Pool size 16; bump for prod. |
| `DATABASE_POOL_SIZE` | `16` | Max connections in the database pool. |
| `BIND_ADDR` | `0.0.0.0:3000` | Use `127.0.0.1:3000` if behind a reverse proxy. |
| `RUST_LOG` | `info` | e.g., `rampart=debug,tower_http=warn,info` |

*Note: SMTP for status-page subscribers is configured inside the app at `/#/settings/smtp`.*

---

## 📡 API Reference

Every UI action is available via REST under `/v1`. Auth is handled via session cookie OR `Authorization: Bearer rmp_<32 chars>` (API keys generated in the UI).

<details>
<summary><strong>Click to expand selected endpoints</strong></summary>

```http
# Health
GET    /healthz
GET    /readyz

# Auth
POST   /v1/auth/login              # { totp_required: true } if 2FA on
POST   /v1/auth/2fa/verify         # second step
GET    /v1/auth/me

# Monitors
GET    /v1/monitors                # list (with hydrated tags)
POST   /v1/monitors                # create
PATCH  /v1/monitors/:id            # partial update
DELETE /v1/monitors/:id
POST   /v1/monitors/:id/pause
POST   /v1/monitors/:id/resume
GET    /v1/monitors/:id/heartbeats?limit=
GET    /v1/monitors/summary?window=86400

# Push monitors (public)
POST   /push/:token                # anything POSTs here counts

# Notifications & Templates
GET    /v1/notifications           # list channels
POST   /v1/notifications           # create channel
GET    /v1/notification-templates  # Liquid templates

# Status Pages
GET    /v1/status-pages
GET    /v1/public/status-pages/:slug              # public read
POST   /v1/public/status-pages/:slug/subscribe

# Incidents & Audit
GET    /v1/incidents/...
POST   /v1/incidents/:id/resolve
GET    /v1/audit-log?limit=&before=&kind=&action= # admin only (action=auth. for security events)
GET    /v1/audit-log/verify                       # admin only — re-walk the hash chain
```
*Full reference available under `backend/crates/rampart-api/src/routes/`.*

</details>

---

## 🧪 Testing

```bash
# Rust workspace
cd backend
DATABASE_URL=postgres://rampart:rampart@localhost:5432/rampart cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

# Frontend
cd frontend
npm test                  # unit tests
npx playwright test       # e2e on Chromium + Firefox + WebKit
```
*CI runs all of the above on push — see `.github/workflows/`.*

---

## 📚 Documentation

📖 **Full documentation site: [pen-pal.github.io/rampart](https://pen-pal.github.io/rampart/)** — searchable, dark-mode, organized by topic. The source lives in [`docs/`](docs/) and the highlights are linked below.

**Getting started & operations**
- [**docs/SETUP.md**](docs/SETUP.md) — Production install, TLS, reverse proxy, and backups.
- [**docs/KUBERNETES.md**](docs/KUBERNETES.md) — Helm chart (OCI), HPA/PDB, service mesh, NetworkPolicy, Prometheus.
- [**docs/WALKTHROUGH.md**](docs/WALKTHROUGH.md) — First-run tour with a screenshot per step.
- [**docs/ARCHITECTURE.md**](docs/ARCHITECTURE.md) — Design decisions and their rationale.
- [**docs/SECURITY-DEBT.md**](docs/SECURITY-DEBT.md) — Accepted RUSTSEC advisories with justification.

**Monitoring**
- [**docs/AGENTS.md**](docs/AGENTS.md) — Remote probe agents: multi-location checks + private-network monitoring.
- [**docs/design/TUNNELING.md**](docs/design/TUNNELING.md) — Tunneling stance: the agent model is private-network reach; no inline proxy.
- [**docs/CRON-JOBS.md**](docs/CRON-JOBS.md) — Cron-job monitoring: run/complete/fail pings, schedule expectations, duration tracking.
- [**docs/SYNTHETICS.md**](docs/SYNTHETICS.md) — Multi-step HTTP transactions: variable extraction + assertions.

**Observability**
- [**docs/design/ERROR-TRACKING.md**](docs/design/ERROR-TRACKING.md) — Sentry-compatible error tracking.
- [**docs/design/TRACES.md**](docs/design/TRACES.md) — OTLP traces / APM, waterfall, service map.
- [**docs/design/LOGS.md**](docs/design/LOGS.md) — OTLP log ingest, full-text search, trace correlation.
- [**docs/design/RUM.md**](docs/design/RUM.md) — Real User Monitoring + browser error capture.
- [**docs/design/PROFILING.md**](docs/design/PROFILING.md) — Continuous profiling (pprof / OTLP / folded) + flamegraphs.
- [**docs/CORRELATION.md**](docs/CORRELATION.md) — The cross-tier link web: log↔trace, error↔trace, trace→profiling, RUM→trace.
- [**docs/INGEST.md**](docs/INGEST.md) — Inbound webhooks + the optional telemetry ingest token.

**Alerting & response**
- [**docs/METRICS.md**](docs/METRICS.md) — Metric ingestion (Prometheus text format), range queries, threshold alert rules.
- [**docs/design/ALERT-RULES.md**](docs/design/ALERT-RULES.md) — Telemetry alert rules over the error/trace/log tiers.
- [**docs/SLOS.md**](docs/SLOS.md) — SLOs with rolling error budgets + burn-rate alerting.
- [**docs/ESCALATIONS.md**](docs/ESCALATIONS.md) — Escalation policies: notification ladders, acknowledge, episode lifecycle.
- [**docs/ON-CALL.md**](docs/ON-CALL.md) — On-call rotations feeding escalation ladders.

**Reference & contributing**
- [**docs/API.md**](docs/API.md) — REST API + the OpenAPI spec (`/openapi.yaml`).
- [**docs/NOTIFICATIONS.md**](docs/NOTIFICATIONS.md) — Notification channels + templating.
- [**CONTRIBUTING.md**](CONTRIBUTING.md) — Scope rules, how to add a probe / channel / migration.
- [**MAINTAINERS.md**](MAINTAINERS.md) — Release workflow and repo settings.

---

## 🤝 Contributing

PRs welcome — read [**CONTRIBUTING.md**](CONTRIBUTING.md) first. The scope rules at the top of that file are load-bearing: they keep "self-hosted uptime monitoring" from drifting into "an everything-platform with three half-working features."

Good first issues are tagged [`good-first-issue`](https://github.com/pen-pal/rampart/labels/good-first-issue) and [`help-wanted`](https://github.com/pen-pal/rampart/labels/help-wanted). Adding a probe or a notification channel? Both have step-by-step guides in CONTRIBUTING.

---

## 🔒 Security

Found a vulnerability? **Don't open a public issue.** Use GitHub's [private vulnerability reporting](https://github.com/pen-pal/rampart/security/advisories/new) so we can ship a patch before it lands on the issue tracker.

CodeQL + Dependabot + cargo-deny + cargo-audit run on every push. Accepted-with-justification advisories are tracked in [**docs/SECURITY-DEBT.md**](docs/SECURITY-DEBT.md) so the acceptance is visible, not buried.

---

## ⚖️ License

[**AGPL-3.0-or-later**](LICENSE).

Modifications you serve over the network must be shared back. Run it on your own hardware, fork it, hack on it — the only thing you can't do is run a closed-source SaaS of it without releasing your changes.

<div align="center">
  <br/>
  <sub><strong>Built with 🦀 Rust by the Rampart community.</strong></sub>
</div>


## ❓ FAQ & Troubleshooting

<details>
<summary><strong>How do I reset the admin password if I'm locked out?</strong></summary>

Since Rampart uses a standard Postgres backend, you can reset the admin password directly via the database if you lose access:

```bash
# 1. Open a shell in your postgres container
docker exec -it rampart-postgres-1 psql -U rampart

# 2. Delete the existing admin user (replace 'admin@local' with your email)
DELETE FROM users WHERE email = 'admin@local';

# 3. Exit psql
\q

# 4. Restart the app and visit the UI to recreate the admin account
docker compose restart rampart
```
</details>

<details>
<summary><strong>How do I backup my data?</strong></summary>

Because Rampart relies entirely on Postgres, backing up is trivial. Just use standard Postgres tools:

```bash
# Dump the database
docker exec rampart-postgres-1 pg_dump -U rampart rampart > backup_$(date +%F).sql

# Restore from a backup
cat backup_2024-01-01.sql | docker exec -i rampart-postgres-1 psql -U rampart rampart
```
*Tip: Automate this with a simple cron job or a tool like [ProBackup](https://github.com/probackup-nl/probackup).*
</details>

<details>
<summary><strong>Why isn't my Generic Webhook firing?</strong></summary>

1. **Check the URL:** Ensure the target URL is reachable from the Rampart container. If you are testing against `localhost`, remember that inside Docker, `localhost` means the container itself. Use `host.docker.internal` or your host's LAN IP.
2. **Check HMAC:** If you enabled HMAC signing, ensure the receiving server is calculating the signature exactly the same way (usually `HMAC-SHA256` of the raw JSON body).
3. **Check Logs:** Run `docker compose logs -f rampart | grep notifier` to see if the HTTP request is returning a non-2xx status code.
</details>

---

## 🗺️ Roadmap

Rampart is feature-complete for 95% of uptime monitoring use cases, but we are actively working on:

- [ ] **Prometheus Exporter:** Expose a `/metrics` endpoint so you can scrape Rampart's internal health and probe latencies into your existing Grafana stack.
- [ ] **Public API Rate Limiting:** Add configurable rate limits to the `/v1` API to prevent abuse if exposed to the public internet.
- [ ] **Webhook Payload Builder:** A visual UI for testing and formatting Liquid templates before saving them to a notification channel.
- [ ] **Read-Only Roles:** Allow team members to view dashboards and status pages without being able to mutate monitors or delete data.

*Have a feature request? [Open a Discussion](https://github.com/pen-pal/rampart/discussions) before submitting a PR to ensure it aligns with our scope.*

---

## 🤝 Contributing & Community

We love contributions! Whether it's fixing a typo, adding a new probe kind, or improving the UI, your help is welcome.

Please read [**CONTRIBUTING.md**](CONTRIBUTING.md) for guidelines on how to set up your dev environment, our coding standards, and how to add new probes or notification channels.

- 🐛 **Found a bug?** [Open an Issue](https://github.com/pen-pal/rampart/issues/new?template=bug_report.md).
- 💡 **Have an idea?** [Start a Discussion](https://github.com/pen-pal/rampart/discussions).
- 💬 **Need help?** Join our [Discord Server](https://discord.gg/rampart) or ask in [GitHub Discussions](https://github.com/pen-pal/rampart/discussions).
- 📋 **What changed?** Read the [**CHANGELOG**](CHANGELOG.md) or browse [tagged releases](https://github.com/pen-pal/rampart/releases). Cutting a release is documented in [`docs/RELEASING.md`](docs/RELEASING.md).
- 📜 **API reference?** [`docs/API.md`](docs/API.md) catalogues every endpoint surfaced by the binary, grouped by URL family with per-route source-file pointers.

---

## ⭐ Star History

If you find Rampart useful, please consider giving it a star! It helps us grow the community and keeps us motivated.

<a href="https://star-history.com/#pen-pal/rampart&Date">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/svg?repos=pen-pal/rampart&type=Date&theme=dark" />
    <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/svg?repos=pen-pal/rampart&type=Date" />
    <img alt="Star History Chart" src="https://api.star-history.com/svg?repos=pen-pal/rampart&type=Date" />
  </picture>
</a>
