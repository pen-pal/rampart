<div align="center">

# Rampart

**Self-hosted uptime monitoring you can actually trust.**

One Rust binary. One Postgres. 28 probe kinds. 130 notification channels. Public status pages. Live SSE updates. Folder + tag routing. Dependency-aware alerts. 2FA. Audit log. Single ~10 MB binary.

[![License](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](LICENSE)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-dea584.svg)](https://www.rust-lang.org/)
[![Postgres](https://img.shields.io/badge/database-Postgres%2014%2B-336791.svg)](https://www.postgresql.org/)
[![Probes](https://img.shields.io/badge/probes-28-brightgreen.svg)](#probes)
[![Notification channels](https://img.shields.io/badge/channels-130-brightgreen.svg)](#notifications)
[![Bundle](https://img.shields.io/badge/binary-~10%20MB-informational.svg)](#why-rampart)

</div>

---

## TL;DR

```bash
git clone https://github.com/rampart-io/rampart.git
cd rampart
docker compose up -d
open http://localhost:3000        # first visit becomes the admin
```

That's the whole install. Migrations run on first boot. The first-visit
signup creates the admin account; further users come through the admin
Users page. No SaaS account, no agent install, no extra services.

---

## Why Rampart

| | |
| --- | --- |
| **One binary** | Frontend embedded via `rust-embed`. ~10 MB stripped release binary. No Node runtime on the host. |
| **Postgres-backed** | The DB you already operate. No sqlite weirdness, no proprietary store. |
| **Pure-Rust crypto** | Web Push (RFC 8291) hand-rolled with `p256` + `aes-gcm`. No `aws-lc-rs` / `openssl` dragged in. |
| **Live, not polled** | Server-Sent Events stream heartbeats to the dashboard; the UI never refreshes from cache. |
| **No telemetry** | Nothing phones home. Self-hosted means self-hosted. |
| **AGPL-3.0** | Modifications you serve over the network must be shared back. Source-available, not source-thrown-over-the-fence. |

Compared to the obvious alternatives:

- **vs. SaaS (Datadog / Pingdom / Site24x7)** — Lives on your hardware,
  no per-monitor pricing, no log-volume bills, no data leaving your
  perimeter.
- **vs. other self-hosted dashboards** — Broader probe catalog
  (databases / banner protocols / Kafka / RADIUS / NTP / Memcached),
  proper tag routing with folder ancestor inheritance, real audit log,
  Postgres instead of sqlite, a single Rust binary instead of a Node
  runtime + headless Chromium.
- **vs. roll-your-own Prometheus blackbox** — Out of the box: status
  pages, incident posting, maintenance windows, dependency-aware
  alerting, 130 outbound channels.

---

## Features at a glance

### Probes

28 kinds, all wired today. Pick a category:

| Category | Kinds |
| --- | --- |
| **HTTP family** | HTTP, Keyword (substring in body), JSON query (JSONPath + expected value) |
| **Network** | TCP, ICMP ping, DNS (A/AAAA/CNAME/MX/TXT/NS/SRV/CAA/SOA), TLS cert days-left, Domain expiry (WHOIS), NTP (SNTPv4) |
| **Databases** | Postgres, MySQL, MSSQL, Redis, MongoDB, Memcached |
| **Messaging / RPC** | gRPC `health.v1`, MQTT, Kafka (ApiVersions handshake), RADIUS |
| **Containers** | Docker daemon |
| **Banner protocols** | SSH, SMTP, IMAP, FTP, POP3 (greeting prefix check, `expect` overridable) |
| **Specialty** | Push (anything POSTing to `/push/:token`), headless-browser keyword (via an external renderer), Steam (A2S) |

Every kind:
- per-monitor interval / timeout / retries / re-alert
- HTTP family: methods, accepted statuses, custom headers/body, follow-redirects, ignore-TLS, proxy
- Soft-fail "warn" status where applicable (e.g. NTP stratum 0 = unsynced)

### Notifications

130 outbound channels. Liquid-templated subject + body, per-channel
cooldown, HMAC-signed Generic Webhook, tag-based auto-routing.

<details>
<summary><strong>Full channel list (130)</strong> — click to expand</summary>

Slack · Discord · Telegram · Teams · Email/SMTP · Pushover · Gotify · ntfy ·
PagerDuty · Mattermost · Rocket.Chat · Twilio SMS · Matrix · Google Chat ·
WeCom · DingTalk · Feishu · Line · Bark · Pushbullet · SendGrid · Resend ·
Brevo · Mailgun · Mailjet · Postmark · Mandrill · SparkPost · Opsgenie ·
PagerTree · Squadcast · Signal · Zulip · Lark · GoAlert · Alerta ·
AlertNow · SIGNL4 · Heii On-Call · ServerChan · PushPlus · PushDeer ·
Aliyun SMS · Mastodon · Pumble · Bitrix24 · Stackfield · Splunk On-Call ·
Grafana OnCall · Home Assistant · ClickSend · 46elks · CallMeBot · Telnyx ·
Notifery · WAHA · Threema · Bale · Pushy · ZohoCliq · SmsManager · SMSEagle ·
Octopush · Whapi · 360messenger · Evolution · Flock · SerwerSMS · SMSPlanet ·
SMSC.ru · Cellsynt · seven.io · GtxMessaging · Onesender · PromoSMS ·
SMSPartner · SMS.ir · FreeMobile · FlashDuty · Teltonika · Kook · Nostr ·
OneBot · OneChat · MAX · Halo PSA · Jira SM · SpugPush · WPush · VK · YZJ ·
Google Sheets · Gorush · Fluxer · Splash · MessageBird · Plivo · Vonage ·
Bandwidth · Webex · Pushcut · SMSGlobal · AlertOps · Spike.sh · Zenduty ·
RingCentral · iLert · Linear · ClickUp · Trello · GitHub Issue · GitLab
Issue · Asana · Notion · Sentry · Rollbar · Honeybadger · Healthchecks.io ·
BetterStack · Statuspage.io · Datadog Events · New Relic Events · AWS SNS ·
Azure Service Bus · GCP Pub/Sub · Apprise gateway · Generic Webhook (HMAC
signed) · Web Push (browser, RFC 8291)

</details>

### Beyond the probe

| Capability | What it does |
| --- | --- |
| **Status pages** | Public, no-login, at `/#/s/:slug`. Worst-status-wins rollup, 90-day uptime per component, incidents with running updates, email subscribers (single-opt-in + unsubscribe token), dark/light theme, clone-existing-page. |
| **Folders + routing** | Nested folders group monitors. Tag a folder/monitor/channel; channels auto-route to monitors sharing a tag. Tag/channel attachments propagate down through sub-folders. Per-monitor exclude pulls a channel off one monitor (exclusion wins). Resolved live per alert. |
| **Dependencies** | A down parent silences dependents so one root cause doesn't paging-storm. Cycle-guarded. |
| **Maintenance** | Time-windowed suppression of probes + notifications. One-shot / daily / weekly recurrence. Pause / resume. Banner on monitor detail. |
| **Tags** | Coloured chips. Dashboard filter (AND semantics, persistent). Inline editor on monitor detail. Admin page with usage counts. |
| **Bulk + clone** | Multi-select dashboard actions (pause / resume / delete / move-to-group). One-click monitor clone. Per-monitor heartbeat CSV export. |
| **Live UI** | Dark / light / system theme. Server-Sent Events live heartbeat stream with connection indicator. Responsive to phones. Folder tree collapses persist across reloads. |
| **Auth** | Session cookie + 2FA (TOTP + 10 single-use recovery codes). API keys (`Authorization: Bearer rmp_…`). Multi-user (admin promote/demote/delete). Self-service password change. |
| **Audit log** | Append-only record of every mutating action. Admin-only viewer with actor / kind / action-prefix filters. Cursor pagination. Click-to-expand payloads. |
| **Retention** | Hourly prune loop for heartbeats + audit log. Windows configurable in the admin UI. |
| **Templates** | Liquid templating for notification subject + body — filters, conditionals, loops. Clone existing templates. |
| **Proxies** | HTTP/SOCKS proxy registry; HTTP-family monitors route through their assigned proxy. |
| **Cert tracking** | Auto-inspect leaf cert on HTTPS monitors every hour. Days-left badge on detail page. |

### What's in the box

```
rampart/
├── backend/                              Rust workspace (axum + sqlx)
│   └── crates/
│       ├── rampart-core                  pure types, no I/O
│       ├── rampart-db                    sqlx repository layer
│       ├── rampart-checker               probe runners (28 kinds)
│       ├── rampart-scheduler             per-monitor tokio tasks + batched writer
│       ├── rampart-notifier              channel fan-out (130 adapters)
│       └── rampart-api                   axum HTTP server (embeds React)
├── frontend/                             Vite + React SPA
├── docs/                                 architecture, setup, security debt
├── Dockerfile                            multi-stage: frontend → rust → debian-slim
├── compose.yaml                          production stack (postgres + rampart)
└── backend/compose.yaml                  dev stack (postgres only)
```

### Out of scope (deliberate)

Multi-region distributed probing, SLO budgets, on-call rotations and
escalation policies, workspace multi-tenancy, APM / RUM / log management.
See [CONTRIBUTING.md](CONTRIBUTING.md#scope-read-this-first) for why,
and [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the rationale
behind every design decision.

---

## Install

### Docker (recommended)

```bash
git clone https://github.com/rampart-io/rampart.git
cd rampart
docker compose up -d           # builds image + starts postgres
open http://localhost:3000
```

Step-by-step walkthrough including TLS, reverse proxy, and backups:
[**docs/SETUP.md**](docs/SETUP.md).

### Single binary (no Docker for the app)

```bash
# 1. Start Postgres on its own
cd backend && docker compose up -d postgres

# 2. Build + run
cd ..
( cd frontend && npm ci && npm run build )
( cd backend && cargo build --release -p rampart-api )
DATABASE_URL=postgres://rampart:rampart@localhost:5432/rampart \
  ./backend/target/release/rampart-api
```

### Dev mode (hot reload)

```bash
# Terminal 1 — Postgres
cd backend && docker compose up -d postgres

# Terminal 2 — backend (auto-rebuilds, no embedded frontend)
cd backend && cargo run -p rampart-api

# Terminal 3 — frontend with HMR
cd frontend && npm run dev          # localhost:5173 proxies API to :3000
```

---

## Configure

All via environment variables. Defaults in `backend/.env.example`.

| Variable | Default | Notes |
| --- | --- | --- |
| `DATABASE_URL` | `postgres://rampart:rampart@localhost:5432/rampart` | Pool size 16; bump for prod |
| `DATABASE_POOL_SIZE` | `16` | |
| `BIND_ADDR` | `0.0.0.0:3000` | Behind a reverse proxy, use `127.0.0.1:3000` |
| `RUST_LOG` | `info` | e.g. `rampart=debug,tower_http=warn,info` |

SMTP for status-page subscribers is set inside the app at
`/#/settings/smtp` (admin only).

---

## API

Every UI action is REST under `/v1`. Auth is the session cookie OR
`Authorization: Bearer rmp_<32 chars>` (API key, generated in the UI —
the raw token is shown once).

<details>
<summary><strong>Selected endpoints</strong> — full reference under <code>backend/crates/rampart-api/src/routes/</code></summary>

```
# Health
GET    /healthz
GET    /readyz

# Auth
POST   /v1/auth/login              { totp_required: true } if 2FA on
POST   /v1/auth/2fa/verify         second step
GET    /v1/auth/me

# Monitors
GET    /v1/monitors                list (with hydrated tags)
POST   /v1/monitors                create
PATCH  /v1/monitors/:id            partial update
DELETE /v1/monitors/:id
POST   /v1/monitors/:id/pause
POST   /v1/monitors/:id/resume
GET    /v1/monitors/:id/heartbeats?limit=
GET    /v1/monitors/summary?window=86400
GET    /v1/monitors/history?per=60

# Push monitors (public)
POST   /push/:token                anything POSTs here counts

# Notifications + templates
GET    /v1/notifications           list (with hydrated tags)
POST   /v1/notifications           create channel
GET    /v1/notification-templates  Liquid templates

# Maintenance
POST   /v1/maintenance-windows
PATCH  /v1/maintenance-windows/:id

# Status pages
GET    /v1/status-pages
GET    /v1/public/status-pages/:slug              public read
POST   /v1/public/status-pages/:slug/subscribe
GET    /v1/public/subscribe/unsubscribe/:token

# Incidents
GET    /v1/incidents/...
POST   /v1/incidents/:id/resolve

# Tags / Folders / API keys / Audit
GET    /v1/tags
GET    /v1/tags/usage
POST   /v1/api-keys                returns the raw token once
GET    /v1/audit-log?limit=&before=&kind=         admin only
```
</details>

---

## Test

```bash
# Rust workspace
cd backend
DATABASE_URL=postgres://rampart:rampart@localhost:5432/rampart \
  cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

# Frontend
cd frontend
npm test                  # unit
npx playwright test       # e2e on Chromium + Firefox + WebKit
```

CI runs all of the above on push — see `.github/workflows/`.

---

## Documentation

- [**docs/SETUP.md**](docs/SETUP.md) — production install, TLS, reverse proxy, backups
- [**docs/ARCHITECTURE.md**](docs/ARCHITECTURE.md) — design decisions and their why
- [**docs/SECURITY-DEBT.md**](docs/SECURITY-DEBT.md) — accepted RUSTSEC advisories with justification
- [**CONTRIBUTING.md**](CONTRIBUTING.md) — scope rules, how to add a probe / channel / migration
- [**MAINTAINERS.md**](MAINTAINERS.md) — release workflow, repo settings

---

## License

[**AGPL-3.0-or-later**](LICENSE). Modifications you serve over the
network must be shared back. Run it on your own hardware, fork it, hack
on it — the only thing you can't do is run a closed-source SaaS of it
without releasing your changes.
