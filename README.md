# Rampart

Self-hosted uptime monitoring. Rust workspace + React frontend in a single binary. Postgres-backed. 21 probe kinds, 130 notification channels (incl. Web Push + AWS SNS / Azure Service Bus / GCP Pub/Sub), public status pages with subscribers + incidents, maintenance windows with recurrence, monitor groups + dependency-aware alert suppression, 2FA, API keys, audit log, multi-user, dark mode, live SSE updates, bulk operations, heartbeat CSV export, TLS cert tracking, push monitors, tag filtering, Liquid templates.

```
rampart/
├── backend/         Rust workspace (axum + sqlx + Postgres)
│   └── crates/
│       ├── rampart-core       — pure types, no I/O
│       ├── rampart-db         — sqlx repository layer
│       ├── rampart-checker    — probe runners (20 kinds)
│       ├── rampart-scheduler  — per-monitor tokio tasks + batched writer
│       ├── rampart-notifier   — channel fan-out (130 adapters)
│       └── rampart-api        — axum HTTP server (embeds React via rust-embed)
├── frontend/        Vite + React SPA
├── docs/            Architecture + setup + notifications reference
├── Dockerfile       multi-stage: frontend → rust → debian-slim runtime
├── compose.yaml     production stack (postgres + rampart)
└── backend/compose.yaml  dev stack (postgres only, run rampart on host)
```

---

## State of things

What works today, against Uptime Kuma parity:

| Area                | Status                                              |
| ---                 | ---                                                 |
| **Probes**          | 21 / 21 implemented — HTTP / keyword / JSON, TCP, ping (ICMP), DNS, push, TLS cert, domain expiry, Postgres / MySQL / MSSQL / Redis / MongoDB, gRPC (health.v1), MQTT, Docker, Steam (A2S), Kafka (ApiVersions), RADIUS, headless-browser keyword (via an external renderer) |
| **Notifications**   | 128 native channels + Apprise gateway + Generic Webhook (130 total). Slack, Discord, Telegram, Teams, Email/SMTP, Pushover, Gotify, ntfy, PagerDuty, Mattermost, Rocket.Chat, Twilio SMS, Matrix, GoogleChat, WeCom, DingTalk, Feishu, Line, Bark, Pushbullet, SendGrid, Resend, Brevo, Mailgun, Mailjet, Postmark, Mandrill, SparkPost, Opsgenie, PagerTree, Squadcast, Signal, Zulip, Lark, GoAlert, Alerta, AlertNow, SIGNL4, Heii On-Call, ServerChan, PushPlus, PushDeer, Aliyun SMS, Mastodon, Pumble, Bitrix24, Stackfield, Splunk On-Call, Grafana OnCall, Home Assistant, ClickSend, 46elks, CallMeBot, Telnyx, Notifery, WAHA, Threema, Bale, Pushy, ZohoCliq, SmsManager, SMSEagle, Octopush, Whapi, 360messenger, Evolution, Flock, SerwerSMS, SMSPlanet, SMSC.ru, Cellsynt, seven.io, GtxMessaging, Onesender, PromoSMS, SMSPartner, SMS.ir, FreeMobile, FlashDuty, Teltonika, Kook, Nostr, OneBot, OneChat, MAX, Halo PSA, Jira SM, SpugPush, WPush, VK, YZJ, Google Sheets, Gorush, Fluxer, Splash, MessageBird, Plivo, Vonage, Bandwidth, Webex, Pushcut, SMSGlobal, AlertOps, Spike.sh, Zenduty, RingCentral, iLert, Linear, ClickUp, Trello, GitHub Issue, GitLab Issue, Asana, Notion, Sentry, Rollbar, Honeybadger, Healthchecks.io, BetterStack, Statuspage.io, Datadog Events, New Relic Events, AWS SNS, Azure Service Bus, GCP Pub/Sub, Web Push (browser, RFC 8291). Per-channel cooldown + HMAC-signed Generic Webhook. |
| **Status pages**    | Public read-only views at `/#/s/:slug`. Worst-status-wins rollup, 90-day uptime per component, incidents banner with running updates, email subscribers (single-opt-in + unsubscribe token), dark/light theme |
| **Groups + deps**   | Cosmetic monitor groups on the dashboard; monitor dependencies with upstream alert suppression (a down parent silences dependents so one root cause doesn't paging-storm), cycle-guarded |
| **Maintenance**     | Time-windowed suppression of probes + notifications, with one-shot / daily / weekly recurrence; admin can pause/resume; banner on monitor detail |
| **Tags**            | Coloured chips, dashboard filter (AND semantics), inline editor on monitor detail |
| **Bulk + clone**    | Multi-select dashboard actions (pause / resume / delete / move-to-group); one-click monitor clone; per-monitor heartbeat CSV export |
| **Live UI**         | Dark / light / system theme; Server-Sent Events live heartbeat stream with a connection indicator; responsive down to phones |
| **Auth**            | Session cookie + 2FA (TOTP + 10 single-use recovery codes), API keys (`Authorization: Bearer rmp_…`), multi-user (admin can create/promote/demote/delete), self-service password change |
| **Audit log**       | Append-only record of mutating actions, admin-only viewer with cursor pagination + resource_kind, action-prefix, and actor filters |
| **Retention**       | Hourly prune loop for heartbeats + audit log; windows configurable in the admin UI |
| **Templates**       | Liquid (Kuma-compatible) for notification subject + body — filters, conditionals, loops |
| **Proxies**         | HTTP/SOCKS proxy registry; HTTP-family monitors route through assigned proxy |
| **Cert tracking**   | Auto-inspect leaf cert on HTTPS monitors every hour; days-left badge on detail page |
| **Edit monitor**    | Full PATCH endpoint + modal exposing common fields (schedule, target, HTTP options) |
| **Bundle**          | Single binary; frontend embedded via `rust-embed`. ~10 MB stripped release binary |

Deliberately out of scope (see [CONTRIBUTING.md](CONTRIBUTING.md#scope-read-this-first)): multi-region distributed probing, SLO budgets, on-call rotations/escalation, workspace multi-tenancy, APM/RUM/log management. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the rationale behind every decision.

---

## Run it (Docker, recommended)

```bash
git clone https://github.com/rampart-io/rampart.git
cd rampart
docker compose up -d           # builds the image + starts postgres
open http://localhost:3000     # first visit becomes the admin
```

Migrations run automatically on first boot. The first-run signup form creates the admin account; subsequent users come through the admin Users page.

Step-by-step walkthrough including TLS, reverse proxy, and backups: [docs/SETUP.md](docs/SETUP.md).

## Run it (single binary, no Docker)

```bash
# 1. Start Postgres on its own
cd backend && docker compose up -d postgres

# 2. Build + run rampart-api
cd ..
( cd frontend && npm ci && npm run build )
( cd backend && cargo build --release -p rampart-api )
DATABASE_URL=postgres://rampart:rampart@localhost:5432/rampart \
  ./backend/target/release/rampart-api
```

## Run it (dev mode with hot reload)

```bash
# Terminal 1 — Postgres
cd backend && docker compose up -d postgres

# Terminal 2 — backend (auto-rebuilds, no embedded frontend)
cd backend && cargo run -p rampart-api

# Terminal 3 — frontend with HMR
cd frontend && npm run dev          # localhost:5173 proxies API to :3000
```

---

## Configuration

All via environment variables. Defaults in `backend/.env.example`.

| Var                  | Default                                            | Notes                                |
| ---                  | ---                                                | ---                                  |
| `DATABASE_URL`       | `postgres://rampart:rampart@localhost:5432/rampart`| Pool size 16; bump for prod          |
| `DATABASE_POOL_SIZE` | `16`                                               |                                      |
| `BIND_ADDR`          | `0.0.0.0:3000`                                     | Behind a reverse proxy, use `127.0.0.1:3000` |
| `RUST_LOG`           | `info`                                             | `rampart=debug,tower_http=warn,info` |

SMTP for status-page subscribers is set inside the app at `/#/settings/smtp` (admin only).

---

## API

Everything the UI does is REST under `/v1`. Authentication is the session cookie OR `Authorization: Bearer rmp_<32 chars>` (API key, generated in the UI).

Selected endpoints:

```
GET    /healthz
GET    /readyz
POST   /v1/auth/login                       — { totp_required: true } if 2FA enabled
POST   /v1/auth/2fa/verify                  — second step
GET    /v1/auth/me
GET    /v1/monitors                         — list (includes hydrated tags)
POST   /v1/monitors                         — create
PATCH  /v1/monitors/:id                     — partial update
DELETE /v1/monitors/:id
POST   /v1/monitors/:id/pause
POST   /v1/monitors/:id/resume
GET    /v1/monitors/:id/heartbeats?limit=
GET    /v1/monitors/summary?window=86400
GET    /v1/monitors/history?per=60
POST   /push/:token                         — public, for push monitors
POST   /v1/notifications                    — create channel
GET    /v1/notification-templates           — Liquid templates
POST   /v1/maintenance-windows
GET    /v1/status-pages
GET    /v1/public/status-pages/:slug        — public read
POST   /v1/public/status-pages/:slug/subscribe
GET    /v1/public/subscribe/unsubscribe/:token
GET    /v1/incidents/...
POST   /v1/incidents/:id/resolve
GET    /v1/tags
POST   /v1/api-keys                         — returns the raw token once
GET    /v1/audit-log?limit=&before=&kind=   — admin only
```

Full reference: skim `backend/crates/rampart-api/src/routes/` — each module is one resource.

---

## Tests

```bash
# Backend unit + integration
cd backend
DATABASE_URL=postgres://rampart:rampart@localhost:5432/rampart \
  cargo test --workspace

# Backend lint
cargo clippy --workspace --all-targets -- -D warnings

# Frontend unit
cd frontend && npm test

# Frontend e2e (Playwright — Chromium + Firefox + WebKit)
cd frontend && npx playwright test
```

CI runs all of the above on push (see `.github/workflows/`).

---

## License

[AGPL-3.0-or-later](LICENSE).
