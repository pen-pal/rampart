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

### Added

#### SLO + reliability analytics (batch 3)

- **SLO breach + recovery notifications.** Closes the loop left open by the SLO schema work below. New `EventKind::SloBreached` / `EventKind::SloRecovered` in `rampart-notifier`; the scheduler's `writer_loop` checks each monitor's rolling SLO uptime after every heartbeat-batch flush and emits a breach (or recovery) alert through the normal notification channels. De-duplicated via a single `slo_breached_at` column (migration `0045_slo_breach_dedup.sql`) so a flapping monitor pages once, not per-batch. New `{{ slo_target_pct }}` / `{{ slo_current_pct }}` template placeholders + purpose-built default subject/body per event kind. The two new kinds appear in the template-editor dropdown.
- **MTBF / MTTR window picker.** `GET /v1/monitors/{id}/reliability` now accepts `?window_days=7|30|90` (whitelist — anything else 400s; defaults to 30). The Reliability card gains a 7d / 30d / 90d segmented control that re-fetches on change.
- **SLO error budget.** New `GET /v1/monitors/{id}/slo/error-budget` returns allowed / used / remaining downtime seconds + remaining %. MonitorDetail renders a horizontal fuel-gauge bar under the SLO card — green (remaining) / red (used) with a "47 min remaining of 144 min budget" label, threshold-coloured (green ≥50% / amber ≥10% / red <10%). 4 new unit tests.
- **Public status-page day-latency drilldown.** The click-day popover on the 90-day strip now shows a CSS-only 24-bar per-hour latency mini-chart (new `GET /v1/public/status-pages/{slug}/day-latency` endpoint backed by `day_hourly_latency`). Rendered for operational/degraded days; per-bar hover tooltip "03:00 — 142ms (47 samples)"; no-data hours muted grey.

#### Importers (3 new — catalog now 10 providers)

- **StatusCake importer.** Maps `test_type` (HTTP / PING / TCP / DNS / SMTP / SSH) onto Rampart kinds.
- **RapidSpike importer.** Maps `monitor_type` (http_check / tcp_check / dns_check / ping_check).
- **Updown.io importer.** Bare-array export; every check is HTTP, promotes to Keyword when `string_match` is set.

#### Frontend

- **i18n Dashboard wiring + locale picker.** Builds on the i18n scaffolding — ~37 Dashboard strings now route through `t(…)`; `en.js` expanded to 49 keys; `es` / `fr` / `de` carry real translations for 11 high-traffic keys (with English fallback); a floating Globe-icon locale picker (beside the theme toggle) cycles all 6 SUPPORTED locales. `ja` / `zh` await native-speaker review.

#### Probes

- **NATS-over-TLS confirmed shipping.** Re-verification found the `tls://` path already works via the ring connector (was mis-described as blocked in an earlier commit message). Cassandra-over-TLS re-verified against scylla 1.6.0 + `main` — still upstream-blocked (no ring-only feature), documented in `docs/SECURITY-DEBT.md`.

_(Earlier post-v0.2.0 work — the monitor-reliability schema, the first
3 importers in this cycle, and AMQPS — is listed below; the SLO batch
above completes the SLO surface those introduced.)_

#### Monitor reliability

- **MTBF / MTTR widget.** New `GET /v1/monitors/{id}/reliability` returns Mean Time Between Failures + Mean Time To Recovery + downtime-event count over the trailing 30 days. Computed by `rampart_db::heartbeats::mtbf_mttr` which walks heartbeats ASC, sums up/down segment durations, and counts up→down / down→up transitions. Frontend renders a Reliability card on the monitor Overview tab — three figures, formatted as "47h 23m" / "4m 12s", with `?`-tooltip help per metric. 4 new unit tests under `rampart-db/tests/heartbeats.rs`.
- **Per-monitor SLO targets.** `Monitor` gains `slo_target_pct` (90.000 — 100.000) + `slo_window_days` (1 — 90) via migration `0044_monitor_slo.sql`. Edit modal exposes the two inputs; Overview tab renders an SLO card next to uptime with current% / target% / status pill (green when current ≥ target, amber within 0.1 pp, red below). `current_slo_uptime_pct` helper in `rampart-db::heartbeats` returns the rolling-window uptime in the same shape as the existing `uptime_pct` helper. _(Notifier dispatch — `EventKind::SloBreached` / `SloRecovered` — has since landed; see the SLO batch at the top of this section.)_

#### Importers (BetterStack / Healthchecks / Cronitor — took the catalog to 7)

- **BetterStack importer.** Maps BetterStack `monitor_type` (status / keyword / keyword_absence / ping / tcp / udp / dns / smtp / pop / imap / playwright) onto Rampart `MonitorKind`. `pronounceable_name` → `Monitor.name`; `check_frequency` (s) → `interval_seconds`; `verify_ssl=false` → `ignore_tls=true`; `keyword_absence` documented as imperfect (negated keyword).
- **Healthchecks.io importer.** Every check maps to `MonitorKind::Push` (Healthchecks is heartbeat-only). UUID stashed in `config.healthchecks_uuid` for reference; `timeout` → `interval_seconds`; `grace` → `timeout_seconds`.
- **Cronitor importer.** Maps `type` (heartbeat / job → Push; check / uptime → Http). `schedule` → `interval_seconds` when it parses as "every N seconds/minutes"; falls back to 60.

#### Probes

- **AMQPS** (RabbitMQ-over-TLS) — the AMQP probe now detects `amqps://` URIs and negotiates TLS via the workspace ring `CryptoProvider`. Lapin's `default-features = false` swapped for explicit `rustls--ring` + `rustls-webpki-roots-certs` so we get TLS without `aws-lc-rs` / `cmake` / `openssl`.
- **NATS-TLS / Cassandra-TLS deferred.** Both upstream crates (`async-nats`, `scylla`) pull `aws-lc-rs` by default with no ring-only escape hatch yet. Probes now reject `tls: true` with a clear error pointing at `docs/SECURITY-DEBT.md` instead of silently running in plaintext. Tracked in `docs/SECURITY-DEBT.md` "Pending TLS gaps" for retry on upstream movement.

#### Public status page

- **Click-day drilldown popover.** Each cell in the 90-day daily-status strip is now interactive (cursor pointer + focus ring + tabIndex + role=button + aria-label). Click / Enter / Space opens a 280px popover anchored under the cell showing the full UTC date ("Mon, March 17 2026"), a status pill colour-keyed to the variant, and any active or resolved incidents whose `[created_at, resolved_at]` window overlaps the day. Esc / click-outside / × closes. Backend-free — uses the existing `incidents` + `incident_history` arrays.

#### Operator deployment

- **Helm chart at `charts/rampart/`.** Chart v0.1.0 wrapping appVersion v0.2.0. Deployment + Service + optional Ingress + optional embedded-Postgres StatefulSet (off by default — production should externalise) + Secret + ConfigMap + standard `_helpers.tpl`. `values.yaml` covers image / replicas / resources / security context / service / ingress / postgres / externalDatabase. `helm lint` clean; `helm template` renders a valid manifest stack.

#### Frontend

- **i18n scaffolding.** New `frontend/src/lib/i18n.js` runtime exporting `t(key, vars?)` + `setLocale` / `getLocale` + a 6-locale `SUPPORTED` list (`en`, `es`, `fr`, `de`, `ja`, `zh`). Dictionaries are flat objects keyed by dotted-namespace strings. Only `en` carries actual translations in this commit; the other five re-export `en` so a partially-translated locale renders cleanly. **Dashboard `t(…)` wiring + locale picker on the floating ThemeToggle are the follow-up — scaffolding only landed here.**

### Notes

- 6 commits since `v0.2.0` (5fa9c1d). Includes pre-commit-hook fix excluding `charts/*/templates/*.yaml` from `check-yaml` (Helm Go-template directives don't parse as YAML).

---

## [0.2.0] — 2026-06-09

The "production-grade" release. Hardens the security surface, doubles the
importer catalog, rebuilds the public status page, ships in-process
HTTP metrics + 1-year heartbeat retention, and lands the operator
deployment artefacts (systemd unit + rotating pg backup script + hardened
compose).

### Added

#### Public status page — full visual redesign

- **Per-component 90-day daily uptime strip.** One coloured cell per day, oldest-left / today-right, with `u`/`d`/`w`/`m`/`n` encoding (operational / down / degraded / maintenance / no-data). Hover title reads "Operational · 14 days ago" etc. Backend `daily_status(monitor, 90)` does a single `GROUP BY date_trunc('day', …)` then pivots into a dense `Vec<u8>`.
- **12-month uptime summary chip row** under each strip — "Jun 99.97% · Jul 100% · …" with SLA-threshold colouring (≥99.9 green / ≥99 amber / <99 red / no-data grey). Backend `monthly_uptime(monitor, 12)` mirrors the daily helper.
- **Default heartbeat retention bumped 90 → 365 days** (migration `0043_retention_one_year.sql`) so the 12-month summary has real data. Idempotent: operator-tuned values preserved.
- **Hero status banner** — big "All Systems Operational" / "Active Incident" / "Service Disruption" / "Scheduled Maintenance" card with one-line subtitle + icon tile.
- **KPI row** — Components count, 90d uptime average, active-incidents count, last-update relative time. Mobile collapses 4-col → 2-col.
- **Active incidents section moved below components**, so the components signal leads. Pinning ranks within the section only — no longer floats above components.
- **Incident history** — last 30 resolved incidents with style-coloured left bar, "Resolved" pill, lasted-duration line ("47 minutes" / "2h 14m" / "3 days").
- **Atom 1.0 + RSS 2.0 feeds** at `/v1/public/status-pages/<slug>/feed.{atom,rss}`. Hand-rolled XML, RFC 3339 + RFC 822 timestamps, proper escape on user-controlled fields. Drops into Feedly / NetNewsWire / Reeder / Slack `/feed`.
- **Tabbed subscribe popover** — Email / RSS / Atom / Webhook. Email uses existing single-opt-in flow; RSS + Atom tabs show the feed URL with Copy + Open buttons; Webhook tab explains operator-side wiring.
- **Live auto-refresh indicator** with pulsing green dot in the brand row.
- **Footer legend** explaining the five strip colours.
- **Admin incident edit** now exposes the "Pin to top" toggle that was previously create-only.

#### Importers (3 new on top of the existing Site24x7)

- **Datadog Synthetics importer.** `rampart-import datadog <export.json>` maps the `(type, subtype)` shape Datadog's `/api/v1/synthetics/tests` returns onto Rampart monitor kinds — covers the `(api, http|tcp|dns|ssl|icmp|grpc|websocket|udp)` cluster plus `(browser, *)`; multi-step synthetics skip with a warn. `body+contains` assertions promote `(api, http)` to `Keyword`.
- **UptimeRobot importer.** `rampart-import uptimerobot <export.json>` maps UptimeRobot's integer-type schema (type 1-5 + sub_type 1-6/99 for port checks). Type 1 → Http, 2 → Keyword (keyword_value carries the substring), 3 → Ping, 4 → Tcp/Ftp/Smtp/Pop3/Imap by sub_type, 5 → Push.
- **Pingdom importer.** `rampart-import pingdom <export.json>` maps Pingdom's string-typed checks (`http`/`tcp`/`dns`/`ping`/`udp`/`pop3`/`smtp`/`imap`/`ssh`). HTTP checks reconstruct the full URL from Pingdom's split `hostname`+`port`+`encryption`+`url` fields. `should_contain` promotes to `Keyword`. `verify_certificate=false` translates to `ignore_tls=true`. Multi-step `transaction` checks skip.
- **Site24x7 importer** — `rampart-import site24x7 <export.json>` CLI (new `[[bin]]` in `rampart-api`) maps a Site24x7 `GET /api/monitors` JSON dump onto Rampart monitors via the existing repository layer. `--dry-run` prints a per-kind breakdown without touching the DB; `--skip-existing` makes the run idempotent on re-import. Documented in `docs/IMPORTERS.md`.

#### Security hardening

- **Security response headers + 2 MiB request body cap.** Every response now ships HSTS / X-Content-Type-Options nosniff / X-Frame-Options SAMEORIGIN / Referrer-Policy / Permissions-Policy (with browser-APIs Rampart doesn't use locked out) / Content-Security-Policy ('self' default with the minimum allow-list for the dashboard's actual remote sources). `RequestBodyLimitLayer(2 MiB)` rejects oversized bodies with 413 (or a clean connection close) before any handler sees them.
- **Per-IP rate limiter on the auth surface.** Token-bucket (capacity 10, refill 1 token / 6 seconds) on `/v1/auth/{register,login,logout,me}` + `/v1/auth/2fa/verify` caps brute-force at ~10 attempts/min per IP without locking out a user who fumbles their password. Opportunistic GC keeps the in-process IP map bounded; X-Forwarded-For + X-Real-IP both honoured.
- **Audit log payload redaction.** `rampart_api::audit::record` walks the JSON payload before persistence and replaces the value of any object key matching a known secret-pattern (`password`, `secret`, `token`, `api_key`, `private_key`, `client_secret`, `auth`, `credential`, `totp`, `recovery`, `vapid`, `smtp_password`, `bind_password`) with `"[redacted]"`.

#### Observability

- **`rampart_http_requests_total` + `rampart_http_request_duration_seconds` histogram on `/metrics`.** In-process middleware records every served request's method + status-class + handler latency. Buckets cover 1 ms → 10 s. Zero new external deps.

#### Operator deployment artefacts

- **Deployment artefacts** under `docs/deploy/`: `rampart.service` (systemd unit wrapping `docker compose up -d --wait` + healthcheck-gated startup), `backup-postgres.sh` (rotating `pg_dump --format=custom` via `docker exec`, 14-day default retention), `README.md` (install + restore + reverse-proxy + non-goals).
- **`compose.yaml` hardened.** Resource limits (Rampart 512 MiB / 1 CPU; Postgres 1 GiB / 1 CPU), `read_only: true` rootfs on Rampart with `tmpfs:/tmp`, `cap_drop: ALL` + `cap_add: NET_RAW`, `security_opt: no-new-privileges:true`, `/readyz`-based healthcheck, `RAMPART_LOG_FORMAT=json` default, `stop_grace_period: 30s`.

#### Tests + repo hygiene

- **8 new e2e specs.** `security-headers.spec.js` (6 specs locking in response-header + 413 body-cap + 429 rate-limit) + `api-keys.spec.js` + `proxies.spec.js`. E2E matrix is now 30 × 5 = 150 cross-browser runs per CI push.
- **Pre-commit hook.** `.pre-commit-config.yaml` at the repo root: pre-commit-hooks hygiene + gitleaks + trufflehog + shellcheck + a local `cargo fmt --all -- --check` hook.

#### UI polish

- **Monitor edit modal redesigned.** Added the missing `.input` CSS class (it was referenced but never defined in `MonitorDetail.jsx`'s style block, so every input fell through to browser defaults); rebuilt the modal as a sectioned form (Basics / Target / HTTP request / Behaviour / Probe config) with sticky header + footer, larger textareas, accent focus rings, and accent-soft toggle pills.
- **Global theme toggle.** Extracted `ThemeToggle` into `src/components/ThemeToggle.jsx` with both inline + floating variants. Floating variant mounted in `App.jsx` so every view (not just the dashboard) carries the light / dark / system cycle button.
- **Notifications channels page tightened.** Form panel constrained to max-width 720, channel-row icons rendered as accent-soft tiles, type/status caption replaced with pill + status-dot.
- **WebSocket probe doc-comment clarifies `wss://`** — the probe already accepted TLS URIs via the workspace-shared ring `CryptoProvider`; the comment + a URL-parser unit test make that explicit so a future refactor that breaks scheme handling fails CI loudly.

### Notes

- 19 commits since `v0.1.0` (29642a2).
- Default heartbeat retention change is migration-driven and idempotent. Installs that operator-tuned the value keep their choice; installs still on the 90-day seed get auto-promoted to 365.

## [0.1.0] — 2026-06-09

First public release. Single Rust binary backed by Postgres, ships a React dashboard embedded into the binary at compile time, covers 38 probe kinds and 130 notification channels.

### Core

- **38 probe kinds** — HTTP, Keyword (substring), JsonQuery, TCP, Ping (ICMP), DNS (A/AAAA/CNAME/MX/TXT/NS/SRV/CAA/SOA), Push (heartbeat-receive), gRPC `health.v1`, TLS handshake + cert expiry, Docker container, Steam (A2S), MQTT, RADIUS, Kafka (ApiVersions handshake), Postgres, MySQL, MSSQL, Redis, MongoDB, Memcached, NTP (SNTPv4), WebSocket (RFC 6455), NATS, LDAP, AMQP (RabbitMQ-compatible), DNS-over-HTTPS (RFC 8484 JSON variant), RDAP (RFC 7480/9082), SNMP v2c GET, Cassandra/ScyllaDB, mDNS service discovery, SSDP/UPnP, Domain expiry (WHOIS), headless-browser, SSH banner, SMTP banner, IMAP banner, FTP banner, POP3 banner.
- **130 notification channels** — Slack, Discord, Telegram, Microsoft Teams, Webhook (HMAC-signed), Email (generic SMTP + SendGrid/Resend/Brevo/Mailgun/Mailjet/Postmark/Mandrill/SparkPost), PagerDuty, Opsgenie, Pushover, Twilio + 26 other SMS providers, Signal, Matrix, Mattermost, Rocket.Chat, Gotify, ntfy, Pushy, Apprise gateway, Splunk On-Call + 18 other incident/on-call platforms, Web Push (RFC 8291 with VAPID), Mastodon/Nostr, Sentry/Rollbar/Honeybadger, AWS SNS / Azure Service Bus / GCP Pub/Sub, Home Assistant, plus 70+ other native adapters in `rampart-notifier/src/channels/`.
- **Scheduler** runs probes off a tokio-based dispatcher with per-monitor concurrency caps and jittered cadence; batched heartbeat writer cuts INSERT round-trips ~100×.
- **Liquid templates** for notification subject + body (`{{ … | filter }}` syntax, conditionals, loops). Preview button on the template editor renders against a fake heartbeat so you can iterate without a probe firing.
- **Folders + tags** for organising large fleets, with routing rules per folder.
- **Dependency-aware alerts** — mark monitor B as depending on monitor A and B suppresses its own outage notification when A is down. Cycle-guarded.
- **Maintenance windows** — schedule one-shot or recurring quiet periods per monitor / folder.
- **Status pages** — public, custom-domainable, dependency-aware. SSE stream means the visitor view updates without reload. Email subscriber sign-up with single-opt-in.
- **Audit log** of every state-changing action; filterable by `kind` / `action` / `actor` / `from` / `to` (RFC 3339 timestamps); CSV export honours the same filters.
- **2FA (TOTP)** with QR enrolment, recovery codes, and a sign-in challenge gate.
- **Push monitor** with regenerable token per monitor for incoming heartbeats from cron jobs.
- **Test-now button** to fire a probe out-of-cycle from the monitor detail view.
- **Bulk actions** on the dashboard (pause/resume/delete/move/attach-channel/detach-channel).
- **Per-monitor heartbeat CSV** export via `/v1/monitors/{id}/heartbeats.csv`.

### API

- `/v1/monitors`, `/v1/monitors/{id}/heartbeats` (cursor pagination via `?before=<rfc3339>&limit=<n>`), `/v1/monitors/{id}/test-now`.
- `/v1/notifications` (channels + counts), `/v1/notifications/{id}/test` (send a probe message through the real provider).
- `/v1/monitors/bulk` — multi-monitor action endpoint.
- `/v1/status-pages` (admin CRUD) + `/v1/public/status-pages/{slug}` (visitor surface) + `/v1/public/status-pages/{slug}/subscribe`.
- `/v1/maintenance-windows`, `/v1/monitor-groups` (folders), `/v1/monitors/{id}/dependencies/{parent_id}`, `/v1/tags`.
- `/v1/audit-log` + `/v1/audit-log/csv` with `kind` / `action` prefix / `actor` UUID / `from` / `to` filters.
- `/v1/auth/...` — register, login, logout, `/me`, TOTP enrol / verify / disable.
- `/v1/api-keys`, `/v1/proxies` (outbound HTTP proxy registry, selectable per-monitor), `/v1/users` (admin).
- `/v1/settings/smtp`, `/v1/settings/retention` (heartbeat + audit retention in days).
- `/healthz` (returns `{"status":"alive","version":"<x.y.z>"}`), `/readyz`, `/metrics` (Prometheus text — `rampart_build_info`, `rampart_monitors{status}`, `rampart_monitors_by_kind{kind}`, `rampart_channels_active`, `rampart_webpush_subscribers`, `rampart_heartbeats_24h{status}`, `rampart_incidents_open`).
- `/v1/stream/heartbeats` — Server-Sent Events live stream.
- `/push/{token}` — public push-monitor heartbeat sink (the token is the auth).
- Hand-curated endpoint reference in [`docs/API.md`](docs/API.md) catalogues every route with source-file pointers.

### Architecture

- Six Rust crates: `rampart-core` (types), `rampart-db` (sqlx repository), `rampart-checker` (probe runners — one file per probe), `rampart-scheduler` (dispatcher), `rampart-notifier` (channel adapters + Liquid template renderer), `rampart-api` (axum HTTP + the main bin).
- Single binary — the frontend bundle is embedded via `rust-embed`. Debug builds read from `frontend/dist`; release builds compile the bytes in.
- Workspace version centralised in `[workspace.package].version`; member crates inherit via `version.workspace = true`. `/healthz`, the Prometheus `rampart_build_info` gauge, the dashboard header pill, the HTTP probe User-Agent, and the Honeybadger notifier payload all read `env!("CARGO_PKG_VERSION")` — single decision point per release.
- sqlx **offline mode** with a committed `.sqlx/` cache. CI builds without bringing up Postgres for compile checks.
- Migrations live in `backend/migrations/000N_*.sql` and run on boot.
- MSRV is **Rust 1.88** (forced by transitive `time` 0.3.47, `base64ct` edition2024). The release Dockerfile uses `rust:1.96-slim-bookworm` for build speed + diagnostics; the MSRV floor is declared in `[workspace.package].rust-version` because that's the contract with library consumers and downstream packagers, not the build pin.
- Pure-Rust crypto stack — `p256` / `aes-gcm` / `hkdf` / `rustls` (ring provider, installed at startup so all rustls callers share one provider) — no OpenSSL, no `aws-lc-rs`, no `cmake`. The `rumqttc` 0.25 and `reqwest 0.13 + default rustls` paths were both attempted and rejected to preserve this invariant; the documented exception is one accepted advisory chain through `rumqttc` 0.24 + `rustls-webpki` 0.102 (CRL-only code paths not reached in default config). See `docs/SECURITY-DEBT.md` for the full ledger.
- `tower-http::request_id` mints an `x-request-id` per request, which `TraceLayer` lifts into a `request_id` span field so every log line emitted inside a handler can be grouped by it. `RAMPART_LOG_FORMAT=json` swaps the human-readable formatter for the JSON one so aggregators (Loki / Datadog / Splunk) index the field as a first-class key.

### Frontend

- Vite 8 + React 19 SPA. **One view per file** under `src/views/`. Inline CSS-in-JSX per view; no Tailwind, no CSS modules.
- All views lazy-loaded via `React.lazy()` inside a single `<Suspense>` boundary; `recharts` and `lucide-react` split into dedicated `vendor-*` chunks. Initial-page JS sits at ~200 kB (gzipped ~63 kB); charts only fetch when a chart view mounts.
- 22 e2e flows on Playwright running against Chromium / Firefox / WebKit + the branded Chrome + Edge channels = 110 cross-browser runs per CI push. Specs cover auth + 2FA, monitor CRUD + dashboard, notification channel + template CRUD, status-page public + admin, subscriber subscribe, folder + dependency edges, maintenance windows, tag routing, dark/light theme, and the per-step walkthrough screenshot generator.
- 32 vitest unit tests covering API helpers, router, theme toggle.
- Light + dark themes that auto-switch from system preference.

### Brand

- Two-tone shield + ECG-pulse mark (`docs/assets/logo.svg`) with matching wordmark lockup (`docs/assets/wordmark.svg`) for GitHub social preview / README hero.
- Favicon (inline SVG data URI — no extra HTTP request from the embedded bundle) and in-app brand mark (Dashboard header + Login card) all share the same mark.
- Step-by-step walkthrough at [`docs/WALKTHROUGH.md`](docs/WALKTHROUGH.md) covers the first-run journey with one labelled screenshot per step (11 PNGs); the screenshots are regenerated end-to-end by `npm run screenshots`.

### Security

- TLS handshake + cert-expiry probe; cert chain validated with `webpki-roots`.
- Argon2 password hashing (`argon2` crate). Failed-login costs a constant-time hash compare regardless of whether the user exists — defends against user-enumeration by timing.
- VAPID-signed Web Push (RFC 8291) using `p256` ECDSA — RFC 8291 §5 known-answer test pinned in `rampart-notifier::channels::webpush_crypto::tests::rfc8291_section5_roundtrip`.
- TOTP (RFC 6238) for 2FA, with recovery codes.
- CSRF, secure-cookie (`HttpOnly`, `SameSite=Strict`, `Secure` on HTTPS), and Argon2 defaults across the auth surface.
- CodeQL + Dependabot enabled. Dependabot groups split routine (minor + patch) / major / security per ecosystem so the routine queue stays mergeable on green.
- One advisory accepted with justification: `RUSTSEC-2026-0049/0098/0099/0104` + two GHSA aliases on `rustls-webpki` 0.102 (via `rumqttc` 0.24) — CRL paths not reached in default config; full rationale in `docs/SECURITY-DEBT.md`.

### CI / tooling

- Backend gates: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --all-targets --no-fail-fast`, `cargo deny check` (advisories + bans + licenses + sources).
- Frontend gates: `npm test` (vitest), `npm run build`, `npx playwright test` across 5 browser projects.
- Docker build/push split into per-arch matrix (amd64 on `ubuntu-latest`, arm64 on the native `ubuntu-24.04-arm` runner with no QEMU); each leg has a `timeout-minutes: 90` hard ceiling; per-arch buildcache refs (`:buildcache-amd64` / `:buildcache-arm64`); a merge job assembles the multi-arch manifest via `docker buildx imagetools create`.
- CodeQL workflow split per language; Rust on `build-mode: none` (the only Rust mode `codeql-action@v4` currently accepts).
- Conflict-labeler workflow creates the `has-conflicts` label idempotently so a clean clone runs without a manual bootstrap step.
- Dependency-bump policy and the living "deferred majors" table documented in [`docs/DEPENDENCIES.md`](docs/DEPENDENCIES.md).

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
