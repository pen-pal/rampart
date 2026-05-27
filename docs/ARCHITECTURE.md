# Rampart · Architecture

Single-binary, single-tenant, Postgres-backed. Six Rust crates in one
workspace; one React SPA embedded into the API binary at compile time.

```
┌────────────────────────────────────────────────────────────────────┐
│  rampart-api (axum binary)                                         │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │  routes/  middleware/  static_assets/  state                 │  │
│  │     │           │                │                           │  │
│  │     │           │  embeds frontend/dist/ via rust-embed      │  │
│  │     │           │                │                           │  │
│  │     ▼           ▼                ▼                           │  │
│  └──────────────────────────────────────────────────────────────┘  │
│           │                                       │                │
│           ▼                                       ▼                │
│   ┌──────────────┐                       ┌──────────────────┐      │
│   │  scheduler   │── spawns ── per-monitor tokio task ──┐   │      │
│   │              │                                      │   │      │
│   │   reload     │◄─── poke from create/update/delete ──┘   │      │
│   │   loop       │                                          │      │
│   └──────────────┘                                          │      │
│           │                                                 │      │
│           ▼ (heartbeat channel)                             ▼      │
│   ┌──────────────┐                          ┌──────────────────┐   │
│   │  writer task │── batched INSERT ──►     │   notifier       │   │
│   │  (256/1s)    │                          │   subscribe →    │   │
│   └──────────────┘                          │   render Liquid →│   │
│           │                                 │   fan-out (126)  │   │
│           ▼                                 └──────────────────┘   │
│   ┌──────────────────────────────────────────────────────────┐     │
│   │                       Postgres                           │     │
│   │  monitors heartbeats notifications tags status_pages …   │     │
│   └──────────────────────────────────────────────────────────┘     │
└────────────────────────────────────────────────────────────────────┘
```

---

## Crates

### `rampart-core` — types

No I/O. Just structs, enums, typed IDs (UUID v7 newtypes). `Monitor`,
`Heartbeat`, `Notification`, `MonitorStatus`, `ChannelKind`, `Tag`,
`MaintenanceWindow`, `StatusPage`, etc. Compiles in < 1 second; every
other crate depends on it.

### `rampart-db` — repository

Thin sqlx wrappers. Raw SQL with `sqlx::query!` / `query_as!` so the
queries are compile-time checked against the live schema (or the
checked-in `.sqlx/` cache in CI). One module per resource. Tests use
`sqlx::test` for per-test isolated databases.

### `rampart-checker` — probes

One file per kind under `src/`: `http.rs`, `tcp.rs`, `dns.rs`,
`ping.rs`, `tls.rs`, `domain.rs`, `postgres.rs`, `mysql.rs`, `mssql.rs`,
`redis.rs`, `mongodb.rs`, `grpc.rs`, `mqtt.rs`, `docker.rs`, `steam.rs`,
`kafka.rs`, `radius.rs`. Each implements `Probe::run(&Monitor) -> Heartbeat`.
The probe layer never touches the database (push monitors and TLS cert
inspection go through the scheduler instead).

Probe-kind dispatch lives in `lib.rs::Probes::run`. Adding a new kind:
new file → register in `Probes::new` → add arm in `run`.

### `rampart-scheduler` — drives probes

One tokio task per active monitor. The task ticks on the monitor's
interval, calls `probes.run(&monitor)`, and sends the heartbeat down an
mpsc channel. A single writer task batches up to 256 heartbeats (or 1
second of wall time, whichever first) and flushes via
`heartbeats::insert_many`. Cuts INSERT round-trips by ~100x at scale.

Special paths:

- **Push monitors** are inverted. The probe-task path doesn't run a
  probe; it reads `last_push_at` from the DB and emits Up / Down based
  on `now - last_push_at` vs `interval + grace`.
- **Maintenance suppression** is checked on every tick before the probe
  runs. Inside an active window → emit a synthetic `Maintenance`
  heartbeat, skip the probe, suppress notifications (a flip in or out
  of Maintenance is treated as non-user-visible).
- **HTTP-family + proxy_id** routes through `HttpProbe::run_with_proxy`
  with a one-shot reqwest client (no pool — keeps the proxy-cache
  problem out of the hot path).
- **TLS cert refresh** runs after a successful HTTP-family probe, at
  most once per hour per monitor; updates `cert_days_left` +
  `cert_subject` + `cert_checked_at` on the monitor row.

Reload-on-change: monitor mutations call `state.poke_scheduler()`,
which bumps a `Notify`. The reload loop diffs current DB rows against
the in-memory task map and starts/stops tasks accordingly.

### `rampart-notifier` — channel fan-out

Each channel is one file under `src/channels/`. The dispatch table in
`channels/mod.rs` maps `ChannelKind` → adapter. The `Channel::send`
trait method takes `(subject, body, &Event)` and returns
`Result<(), ChannelError>`.

Templating: subject + body are rendered through `template::render` using
the Liquid dialect (Kuma-compatible). Filters, conditionals, and loops
are supported. Variables available: `monitor.name / url / kind / id /
hostname / port`, `status`, `prev_status`, `latency_ms`, `status_code`,
`msg`, `retries`, `ts`.

When a monitor is missing a configured template, defaults from
`template::default_subject` / `default_body` are used. The notifier
service module wires this together and walks `monitor_notifications`
per heartbeat event.

### `rampart-api` — axum binary

One module per HTTP resource under `src/routes/`. The single binary
serves `/v1/*` API routes, `/push/:token` for push-monitor heartbeats,
`/healthz` + `/readyz`, and a fallback that serves the embedded SPA
(via `rust_embed::Embed` against `../../../frontend/dist/`).

Middleware order:

1. `set_request_id` (UUID v4 per request)
2. `trace` (structured logs)
3. `propagate_request_id`
4. `compression_gzip`
5. `timeout` (15 s)
6. `cors` (relaxed; tighten for production)
7. Per-route `require_session` for `/v1/*` except `/v1/auth/*` and
   `/v1/public/*`. Admin-only subtrees additionally layer
   `require_admin`.

Auth accepts:

- Browser session cookie (HttpOnly, SameSite=Strict, 14 d TTL)
- API key bearer token: `Authorization: Bearer rmp_<32 base62 chars>`
  — stored as SHA-256 hex in `api_keys.key_hash`, last 8 chars in
  `key_prefix` for UI fingerprinting.

2FA: when `users.totp_enabled`, the login response defers session
creation and returns `{ totp_required, challenge_token }`. The frontend
posts to `/v1/auth/2fa/verify` with the challenge + code (TOTP or
recovery). Recovery codes are SHA-256-hashed, single-use.

---

## Database

Postgres 16+, single schema, single tenant. Migrations live in
`backend/migrations/0001_*.sql` … and beyond — `sqlx::migrate!` runs
them on every boot. No down migrations; rolling forward only.

Key tables:

| Table                          | Purpose                                          |
| ---                            | ---                                              |
| `users`                        | email, argon2 password hash, `totp_secret`, `totp_enabled`, `is_admin` |
| `sessions`                     | server-side sessions keyed by UUID v4            |
| `api_keys`                     | SHA-256 hash + 8-char prefix + scopes            |
| `totp_recovery_codes`          | hashed, single-use                               |
| `monitors`                     | 20 kinds, scheduling, HTTP opts, push_token, cert snapshot |
| `monitor_tags`                 | M2M with `tags`                                  |
| `heartbeats`                   | append-only, batched-write                       |
| `notifications`                | channel rows                                     |
| `notification_templates`       | Liquid subject + body                            |
| `monitor_notifications`        | M2M routing                                      |
| `maintenance_windows` + `_monitors` | suppression schedule                        |
| `proxies`                      | HTTP/SOCKS upstreams                             |
| `status_pages` + `_monitors`   | public projection                                |
| `incidents` + `_updates`       | per-page announcements                           |
| `status_page_subscribers`      | email + unsubscribe token                        |
| `settings`                     | key/value JSON (currently `smtp`)                |
| `audit_log`                    | append-only mutating-action record               |

Heartbeats grow unbounded today — pruning + partition rotation is the
next reliability gap.

---

## Frontend

Vite + React. Hash-based router (`#/`, `#/monitor/:id`, `#/status-page`,
`#/s/:slug` public viewer, `#/notifications`, `#/maintenance`,
`#/users`, `#/api-keys`, `#/security`, `#/proxies`, `#/audit`,
`#/settings/smtp`). Public viewer (`#/s/:slug`) bypasses the
`App.jsx` auth gate.

State management: vanilla React hooks. The `useApi` hook in
`src/lib/api.js` is the only abstraction — fire-and-render with
optional polling. After mutations, components call
`window.location.reload()` rather than threading manual state
invalidation. Adequate for tens of monitors; would be replaced by
react-query / swr if the dataset grew.

Build output: `frontend/dist/`. The API binary embeds this folder at
compile time via `rust-embed`. Debug builds read it from disk so
frontend changes don't require a backend rebuild; release builds bake
it in.

---

## Adding things

| Adding…             | Files to touch                                                |
| ---                 | ---                                                           |
| New probe kind      | `rampart-core/src/monitor.rs::MonitorKind` + `rampart-checker/src/<kind>.rs` + `rampart-checker/src/lib.rs::Probes` + wizard config in `NewMonitorWizard.jsx` |
| New channel         | `rampart-core/src/notification.rs::ChannelKind` + migration `ALTER TYPE channel_kind ADD VALUE` + `rampart-notifier/src/channels/<name>.rs` + dispatch in `mod.rs` + `SUPPORTED` + `ConfigForm` in `Notifications.jsx` |
| New route           | one module under `rampart-api/src/routes/` + wire in `routes/mod.rs::v1_public` or `v1_protected` |
| Schema change       | new `backend/migrations/NNNN_<name>.sql`; for enum extensions use `ALTER TYPE ... ADD VALUE IF NOT EXISTS` |
