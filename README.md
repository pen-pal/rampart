<div align="center">

<img src="docs/assets/logo.svg" alt="Rampart Logo" width="100" />

# Rampart

### Self-hosted uptime monitoring you can actually trust.

**One Rust binary. One Postgres. 31 probe kinds. 130 notification channels.**<br/>
Public status pages • Live SSE updates • Folder & tag routing • Dependency-aware alerts • 2FA • Audit logs.

[![License](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](LICENSE)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-dea584.svg?logo=rust)](https://www.rust-lang.org/)
[![Postgres](https://img.shields.io/badge/database-Postgres%2014%2B-336791.svg?logo=postgresql)](https://www.postgresql.org/)
[![Probes](https://img.shields.io/badge/probes-31-brightgreen.svg)](#-31-probe-kinds)
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

---

## ✨ Features at a Glance

### 🔍 31 Probe Kinds
Every probe supports per-monitor intervals, timeouts, retries, and re-alerts. 

| Category | Supported Kinds |
| :--- | :--- |
| 🌐 **HTTP Family** | HTTP, Keyword (substring), JSON query (JSONPath + expected value) |
| 📡 **Network** | TCP, ICMP ping, DNS (A/AAAA/CNAME/MX/TXT/NS/SRV/CAA/SOA), TLS cert days-left, Domain expiry (WHOIS), NTP (SNTPv4) |
| 🗄️ **Databases** | Postgres, MySQL, MSSQL, Redis, MongoDB, Memcached |
| 📢 **Messaging / RPC** | gRPC `health.v1`, MQTT, Kafka (ApiVersions handshake), NATS, RADIUS |
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
| 🔐 **Auth & Security** | Session cookie + 2FA (TOTP + 10 recovery codes). API keys (`rmp_…`). Multi-user with RBAC. Append-only audit log. |
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
│       ├── rampart-checker               # probe runners (31 kinds)
│       ├── rampart-scheduler             # per-monitor tokio tasks + batched writer
│       ├── rampart-notifier              # channel fan-out (130 adapters)
│       └── rampart-api                   # axum HTTP server (embeds React)
├── frontend/                             # Vite + React SPA
├── docs/                                 # architecture, setup, security debt
├── Dockerfile                            # multi-stage: frontend → rust → debian-slim
├── compose.yaml                          # production stack (postgres + rampart)
└── backend/compose.yaml                  # dev stack (postgres only)
```

> **Out of scope (deliberate):** Multi-region distributed probing, SLO budgets, on-call rotations, workspace multi-tenancy, APM / RUM / log management. See [CONTRIBUTING.md](CONTRIBUTING.md#scope-read-this-first) for why.

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
GET    /v1/audit-log?limit=&before=&kind=         # admin only
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

- [**docs/SETUP.md**](docs/SETUP.md) — Production install, TLS, reverse proxy, and backups.
- [**docs/ARCHITECTURE.md**](docs/ARCHITECTURE.md) — Design decisions and their rationale.
- [**docs/SECURITY-DEBT.md**](docs/SECURITY-DEBT.md) — Accepted RUSTSEC advisories with justification.
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
