# Contributing to Rampart

Thanks for your interest. Rampart is a self-hosted uptime monitor for **homelabs, indie devs, and small teams**, shipping as a single binary backed by Postgres. Before opening an issue or a PR, please read the scope section below — it'll save us all time.

## Scope (read this first)

Rampart is **not** an enterprise observability platform. The following are deliberately out of scope and will be closed as `wontfix`:

- Multi-region distributed probing
- SLO targets / error budgets
- On-call rotations + escalation policies
- AI anomaly detection, AI-generated post-mortems, AIOps
- Workspace multi-tenancy (no `workspace_id` columns anywhere)
- APM tracing, RUM, log management, server-agent metrics
- Kubernetes / cloud-provider scanners

These were considered and removed during the v1 → v2 design pivot. The full rationale is in [`docs/DESIGN.md`](docs/DESIGN.md).

**A useful test:** *"would a solo operator or a small-team SRE say 'that's not what I came here for'?"* If yes, it doesn't belong here.

In scope:
- The 17 unimplemented probe runners (DNS, ping, push, gRPC, TLS, Docker, databases, MQTT, Steam, RADIUS, Kafka, domain WHOIS)
- The `rampart-notifier` crate (Slack / Discord / Email / Webhook fan-out)
- Auth (session-based, argon2)
- Public status-page renderer
- Incidents + maintenance REST APIs
- Data importers (SQLite / JSON exports from existing monitors)
- Anything in the [README "What to build next"](README.md#what-to-build-next) section

## Project layout

```
backend/       Rust workspace, 5 crates (rampart-core/-db/-checker/-scheduler/-api)
frontend/      Vite + React, 4 dashboard views
docs/          Design rationale and architecture
LICENSE        AGPL-3.0-or-later
README.md      Setup + how to run
```

Per-area conventions live in:
- [`backend/HACKING.md`](backend/HACKING.md) — crate boundaries, sqlx patterns, probe trait, scheduler design, how to add a new monitor kind end-to-end
- [`frontend/HACKING.md`](frontend/HACKING.md) — design tokens, inline-CSS-in-JSX rationale, component patterns

Both files are required reading before touching code in their respective area.

## Setup

See the [README](README.md#run-it-single-binary-recommended) for the canonical setup. Quick version:

```bash
# Postgres
cd backend && docker compose up -d postgres
cp .env.example .env

# Frontend bundle (one-shot — only re-run when you change UI)
cd ../frontend && npm install && npm run build

# Backend
cd ../backend && cargo run -p rampart-api
```

Open <http://localhost:3000>.

For fast UI iteration, run `npm run dev` in `frontend/` and hit `:5173` — Vite proxies `/v1/*` to `:3000`.

## Conventions

- **Edition** 2021, **MSRV** 1.78
- **Time** via the `time` crate (not chrono), `OffsetDateTime` in UTC
- **IDs** are UUIDv7 wrapped in per-entity newtypes (`MonitorId`, `IncidentId`, etc.) — see `rampart-core/src/ids.rs`. Never pass a raw `Uuid` between functions when a typed ID exists.
- **DB access** via `sqlx::query!` / `sqlx::query_as!` (compile-time checked). Raw SQL, no query builder DSL.
- **Domain errors** in `rampart-core::CoreError`, db errors in `rampart-db::DbError`, api errors in `rampart-api::error::ApiError`. Don't leak `sqlx::Error` to clients.
- **One probe per file** in `rampart-checker/src/`. Don't unify them prematurely.
- **Frontend CSS** is inline CSS-in-JSX per view (`<style>{css}</style>`). No Tailwind, no CSS modules. Each view is self-contained on purpose.
- **No AI features.** Removed in the v2 pivot; do not re-introduce.

## How to add a new monitor kind

End-to-end, in order:

1. **Migration:** `ALTER TYPE monitor_kind ADD VALUE IF NOT EXISTS 'newthing';` in a new `backend/migrations/000N_*.sql`.
2. **Enum variant:** add to `MonitorKind` in `rampart-core/src/monitor.rs`.
3. **Probe:** new file in `rampart-checker/src/newthing.rs` implementing `Probe`. Always return `Heartbeat`, never `Result` — failures become heartbeats with `status = Down` and a descriptive `msg`.
4. **Dispatch:** add the match arm in `rampart-checker/src/lib.rs::Probes::run`.
5. **Wizard UI:** add to the `types` array in `frontend/src/views/NewMonitorWizard.jsx` with icon + description. Drop the `stub: true` flag once it actually probes.
6. **Field requirements:** update `fieldsFor()` in the same file.

More detail in `backend/HACKING.md`.

## Tests

```bash
cd backend
cargo test --workspace          # requires DATABASE_URL pointing at a migrated PG
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
```

CI builds use `SQLX_OFFLINE=true` against a cached `.sqlx/` directory. If you change a query, regenerate the cache:

```bash
cargo sqlx prepare --workspace
git add .sqlx
```

Frontend:

```bash
cd frontend
npm run build                   # vite build, surfaces JSX errors
```

## Pull requests

- **Branch from `main`.** Name it `kind/short-description` (e.g. `probe/dns`, `fix/scheduler-leak`, `docs/sqlx-cache`).
- **One concern per PR.** A feature and a refactor go in separate PRs.
- **Commit messages** — short imperative subject (≤70 chars), body explains the *why*. Reference issues like `Fixes #42` when applicable.
- **No `--no-verify`.** If a hook fails, fix the underlying issue.
- **Don't bump versions** in PRs — that's done at release time.

## Reporting bugs / requesting features

Before opening an issue:

1. Check if it's a known stub (see the README's "Not yet" list — many probes deliberately return `Down "not yet implemented"`).
2. Check if it falls in the rejected scope above.
3. For bugs: include `rampart-api` version, Postgres version, the relevant log lines, and steps to reproduce. The log lines from `rampart_scheduler` are usually the most informative.
4. For features: explain the use case in concrete terms. "Some other tool has X" is not a use case; "I run a homelab and need to be told when my Plex server stops responding" is.

## License

By contributing, you agree your contributions will be licensed under [AGPL-3.0-or-later](LICENSE), the same license as the project. There is no CLA. We use AGPL deliberately to keep the project + its derivatives in the open — if you can't accept that, this isn't the project for you, and that's fine.
