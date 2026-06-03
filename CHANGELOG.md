# Changelog

All notable changes to Rampart are recorded here.

The format is based on [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/)
and the project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html):

- **MAJOR** — incompatible API changes, schema migrations that require a manual step, or removal of a probe / channel kind.
- **MINOR** — new probe kinds, new notification channels, new endpoints, new dashboard views — anything additive.
- **PATCH** — bug fixes, security fixes, dependency bumps, documentation, packaging.

The version baked into the binary (and surfaced on `/healthz` + the dashboard header) is the single source of truth — it inherits from `[workspace.package].version` in `backend/Cargo.toml`. Frontend `package.json` tracks the same value.

For the procedure to cut a release see [`docs/RELEASING.md`](docs/RELEASING.md).

---

## [Unreleased]

### Brand
- Two-tone shield + ECG-pulse logo (`docs/assets/logo.svg`) replaces the earlier teal-shield-with-R mark.
- New `docs/assets/wordmark.svg` lockup for GitHub social-preview / README hero contexts.
- Favicon and in-app brand mark (Dashboard header + Login card) re-shaped to match.

### Versioning
- Workspace version centralised in `[workspace.package].version`; member crates inherit via `version.workspace = true`.
- `/healthz` returns the version (`{"status":"alive","version":"<x.y.z>"}`); the dashboard header pill is now dynamic instead of a hard-coded string.
- Prometheus `rampart_build_info` gauge interpolates `CARGO_PKG_VERSION` instead of a hard-coded literal.

### CI / Tooling
- E2E matrix now runs Playwright across Chromium, Firefox, WebKit, and the branded Chrome + Edge channels (17 specs × 5 projects = 85 runs per push).
- Dependabot groups: `cargo-routine` + `cargo-security`, `npm-routine` + `npm-security`, monthly grouped runs for GitHub Actions and Docker.
- CodeQL workflow split per language; Rust stays on `build-mode: none` until upstream supports `manual`.
- Conflict-labeler workflow creates the `has-conflicts` label idempotently so a clean clone runs without a manual bootstrap step.

---

## [0.1.0] — 2026-06-03

First public release. Everything below this line is what the binary ships today.

### Core

- **29 probe kinds.** HTTP/HTTPS, TCP, DNS (A/AAAA/CNAME/MX/TXT/NS/SRV/CAA/SOA), Ping (ICMP), TLS-handshake + cert expiry, Postgres, MySQL, MSSQL, MongoDB, Redis, gRPC health, MQTT, AMQP, SMTP banner, IMAP banner, POP3 banner, FTP banner, SSH banner, JSON-API, RSS feed freshness, JSON value, JSON schema, keyword match, Docker container, push (heartbeat-receive), browser (synthetic), RADIUS auth, kafka-broker, websocket.
- **130 notification channels.** Slack, Discord, Telegram, Microsoft Teams, Webhook (custom), Email (SMTP), PagerDuty, Opsgenie, Pushover, Twilio SMS, Signal, Matrix, Mattermost, Rocket.Chat, Gotify, ntfy, Pushy, Apprise bridge, Splunk On-Call, Webex, Zulip, Web Push (RFC 8291) + 108 native adapters in `rampart-notifier/src/channels/`.
- **Scheduler** runs probes off a tokio-based dispatcher with per-monitor concurrency caps and jittered cadence.
- **Liquid templates** for notification subject + body (`{{ … | filter }}` syntax, conditionals, loops). Preview button on the template editor renders against a fake heartbeat so you can iterate without a probe firing.
- **Folders + tags** for organising large fleets, with routing rules per folder.
- **Dependency-aware alerts** — mark monitor B as depending on monitor A and B suppresses its own outage notification when A is down.
- **Maintenance windows** — schedule one-shot or recurring quiet periods per monitor / folder.
- **Status pages** — public, custom-domainable, dependency-aware. SSE stream means visitor view updates without reload.
- **Audit log** of every state-changing action; CSV export.
- **2FA (TOTP)** with QR enrolment, including the recovery codes flow.
- **Push monitor** with regenerable token per monitor.
- **Test-now button** to fire a probe out-of-cycle from the monitor detail view.

### API

- `/v1/monitors`, `/v1/monitors/{id}/heartbeats` (with cursor pagination via `before=<rfc3339>`), `/v1/monitors/{id}/test-now`.
- `/v1/notifications` (channels + counts), `/v1/notifications/test` (send a probe message).
- `/v1/monitor-channels/bulk` — attach / detach a notification channel across many monitors in one request.
- `/v1/status-pages`, `/v1/maintenance`, `/v1/folders`, `/v1/tags`, `/v1/dependencies`.
- `/v1/audit?format=csv` — CSV export of the audit log.
- `/v1/auth/...` — register, login, logout, `/me`, TOTP enrol / verify / disable.
- `/healthz`, `/readyz`, `/metrics` (Prometheus text).
- SSE stream at `/v1/events` for live heartbeat fan-out.

### Architecture

- Six Rust crates: `rampart-core` (types), `rampart-db` (sqlx repository), `rampart-checker` (probe runners — one file per probe), `rampart-scheduler` (dispatcher), `rampart-notifier` (channel adapters + Liquid template renderer), `rampart-api` (axum HTTP + the main bin).
- Single binary — the frontend bundle is embedded via `rust-embed`. Debug builds read from `frontend/dist`; release builds compile the bytes in.
- sqlx **offline mode** with a committed `.sqlx/` cache. CI builds without bringing up Postgres for compile checks.
- Migrations live in `backend/migrations/000N_*.sql` and run on boot.
- MSRV is **Rust 1.88** (forced by transitive `time` 0.3.47, `base64ct` edition2024). Release Dockerfile pins `rust:1.88` to match.
- Pure-Rust crypto stack — `p256` / `aes-gcm` / `hkdf` / `rustls` — no OpenSSL / aws-lc-rs / cmake dependency.

### Frontend

- Vite + React SPA, **one view per file** under `src/views/`. Inline CSS-in-JSX per view; no Tailwind, no CSS modules.
- 17 e2e flows on Playwright running against Chromium / Firefox / WebKit + the branded Chrome + Edge channels.
- 32 vitest unit tests covering API helpers, router, theme toggle.
- Light + dark themes that auto-switch from system preference.

### Security

- TLS handshake + cert-expiry probe; cert chain validated with `webpki-roots`.
- Argon2 password hashing (`argon2` crate).
- VAPID-signed Web Push (RFC 8291) using `p256` ECDSA — no openssl linkage.
- TOTP (RFC 6238) for 2FA.
- CSRF, secure-cookie, and same-site-strict defaults on the auth surface.
- CodeQL + Dependabot enabled; advisory log in `docs/SECURITY-DEBT.md`.

### Out of scope (deliberately)

The following were considered and removed during the v1→v2 design pivot. Issues / PRs touching these are closed as `wontfix`:

- Multi-region distributed probing
- SLO targets / error budgets
- On-call rotations + escalation policies
- AI anomaly detection / AIOps / AI post-mortems
- Workspace multi-tenancy
- APM / RUM / log management / agent-based metrics
- Kubernetes / cloud-provider scanners

Full rationale in [`docs/DESIGN-ORIGINAL.md`](docs/DESIGN-ORIGINAL.md).

---

[Unreleased]: https://github.com/pen-pal/rampart/compare/v0.1.0...HEAD
[0.1.0]:      https://github.com/pen-pal/rampart/releases/tag/v0.1.0
