# Rampart — design rationale

This is the long-form "what Rampart is, what it isn't, and why" doc. If you just want to run it, see the [root README](../README.md). If you're contributing code, see the per-area `HACKING.md` files in `backend/` and `frontend/`.

Rampart is a self-hosted uptime monitor for **homelabs, indie devs, and small teams**. One process, one Postgres database, one place to put your monitors and your public status page.

This is **not** an enterprise observability product. No multi-region probing, no SLO budgets, no on-call rotations, no AI anomaly detection. Those exist in other tools and belong to a different product category.

## Design goals

1. **Single-binary deploy.** `rampart-api` embeds the React frontend; `cargo build --release` produces one executable that serves both API and UI on the same port. No reverse proxy, no static-asset server, no orchestration to wire frontend to backend.
2. **Postgres, not SQLite.** Embedded SQLite is cheap to ship and convenient at first, but causes pain at scale — file-locking, foreign-key edge cases, Docker volume permission errors, and a difficult upgrade story past a few dozen monitors. Postgres trades a one-time setup cost for predictable behaviour at every size after.
3. **Boring monitoring, done well.** HTTP, TCP, DNS, ping, TLS / certificate expiry, domain (WHOIS) expiry, push (heartbeat). The mainstream 80% — no exotic protocols.
4. **Public status pages, free.** Subscribers, custom domains, SVG badges with logos. The full feature without a paid tier.
5. **Notification templating built in.** Every channel renders bodies through the same template — no per-channel "edit the message in 8 places" friction.
6. **Recurring maintenance with cron + weekday/monthly patterns.** Not "single window once a week" — actual scheduling.

## Architecture

```
┌──────────────┐    ┌──────────────┐    ┌──────────────┐
│ rampart-api  │    │ rampart-     │    │ rampart-     │
│  (axum)      │    │ checker      │    │ scheduler    │
│              │    │              │    │              │
│  REST + UI   │    │  Probe trait │    │  per-monitor │
│  (embedded   │    │  + per-kind  │    │  tasks +     │
│  React)      │    │  runners     │    │  batch writer│
└──────┬───────┘    └──────┬───────┘    └──────┬───────┘
       │                   │                   │
       └───────────┬───────┴───────────┬───────┘
                   │                   │
                   ▼                   ▼
              ┌──────────────────────────────┐
              │   rampart-db (sqlx → PG)     │
              └──────────────────────────────┘
                          ▲
                          │
                   ┌──────┴──────┐
                   │ rampart-    │
                   │ notifier    │
                   │  (planned)  │
                   │             │
                   │ Slack /PD/  │
                   │ Discord etc │
                   └─────────────┘

rampart-core: I/O-free domain types, depended on by every other crate.
```

**Workspace crates (all in `backend/crates/`):**

| Crate | Role | Status |
|---|---|---|
| `rampart-core`     | I/O-free domain types. Monitor, Heartbeat, Incident, Notification, typed IDs, validation. | ✅ |
| `rampart-db`       | sqlx-backed repository over Postgres. Compile-time-checked SQL. | ✅ |
| `rampart-checker`  | `Probe` trait + per-kind runners. HTTP (handles plain/keyword/json_query) and TCP fully implemented; 17 other kinds return `Down "not yet implemented"`. | partial |
| `rampart-scheduler` | Per-monitor tokio tasks, mpsc to a batch writer (256 rows or 1s), reload-on-mutation via `tokio::Notify`. | ✅ |
| `rampart-api`      | axum binary. Embeds the React bundle via `rust-embed`. Owns routing. | ✅ |
| `rampart-notifier` | Subscribe to status flips, fan out to Slack/Discord/Email/Webhook via shared template renderer. | planned |
| `rampart-status`   | Public status page renderer (custom domains). | planned |

## Data model — what's in `backend/migrations/0001_initial.sql`

| Table | Purpose |
|---|---|
| `users`, `sessions` | Auth |
| `monitors` | Probe definitions |
| `heartbeats` | Check results (high volume, partitioned-friendly) |
| `tags`, `monitor_tags` | Labeling (with optional values) |
| `proxies` | Outbound HTTP/SOCKS proxies for probes that need them |
| `notifications` | Channel configs |
| `monitor_notifications` | Which channels per monitor (flat fan-out, no priority) |
| `notification_templates` | Reusable message templates shared across channels |
| `maintenance`, `monitor_maintenance` | Windows including recurring + cron strategies |
| `status_pages`, `status_page_groups`, `status_page_components` | Public pages |
| `status_page_subscribers` | Email / SMS subscriptions |
| `incidents`, `incident_updates` | Status-page announcements |
| `api_keys` | Programmatic access |
| `status_badges` | Per-monitor SVG badges with logos |
| `settings` | Workspace key-value |
| `audit_log` | Who-did-what |

Monitor kinds covered (20):

```
http  keyword  json_query  tcp  ping  dns  push  grpc  tls
docker  steam  mqtt  radius  kafka
postgres  mysql  mssql  redis  mongodb
domain
```

## What's been cut (do not re-introduce)

Rampart's design went through a v1 → v2 pivot. The v1 scaffold was enterprise-shaped and didn't serve the actual audience. The following were removed deliberately:

| Removed | Why |
|---|---|
| `workspaces` + `workspace_members` | Single-tenant by design. No `workspace_id` columns anywhere. No `X-Workspace-Id` header. |
| `monitor.regions` column | Single-install scope. Probes run from one place. |
| `monitor.slo_target` + `SloTarget` type | SLOs are enterprise scope. |
| `monitor.ai_anomaly_enabled` + `anomaly_baselines` table | Out of scope. |
| `monitor.auto_failover_enabled` | Out of scope. |
| `monitor_dependencies` table | Interesting but not core. |
| `oncall_rotations` + `oncall_overrides` | Solo operators don't need rotations. |
| `routing_rules` (priority-ordered + escalation) | Replaced with flat fan-out via `monitor_notifications`. |
| Incident timeline + action items + AI summary fields | Incidents here are status-page announcements, not investigation records. |
| `Region` enum, `region.rs` | Deleted. |

Net: about 40% smaller surface area than v1, much closer to what the target audience actually uses.

**Scope test for any feature request:** *would a solo operator or a small-team SRE say "that's not what I came here for"?* If yes, it doesn't belong.

## sqlx compile-time check

The repository queries use `sqlx::query!` macros, which validate against the live database at build time. Two options for CI:

1. **Live DB:** set `DATABASE_URL` in the build environment with a Postgres instance that has the migrations applied.
2. **Cached:** run `cargo sqlx prepare --workspace` locally and commit `.sqlx/`. Then set `SQLX_OFFLINE=true` in CI.

## Roadmap

See the [root README](../README.md#what-to-build-next) for the prioritised next-steps list.
