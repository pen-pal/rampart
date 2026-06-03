# Contributing to Rampart

Thanks for your interest in Rampart! 🦀

Rampart is a self-hosted uptime monitor built for **homelabs, indie devs, and small teams**, shipping as a single binary backed by Postgres. 

Before opening an issue or a PR, please read the scope section below carefully — it will save us all a lot of time.

---

## 🎯 Scope (Read This First)

Rampart is **not** an enterprise observability platform. We have a strict focus.

### 🚫 Out of Scope
The following features were considered and deliberately removed during the v1 → v2 design pivot. PRs or issues for these will be closed as `wontfix`:

- Multi-region distributed probing
- SLO targets / error budgets
- On-call rotations + escalation policies
- AI anomaly detection, AI-generated post-mortems, AIOps
- Workspace multi-tenancy (no `workspace_id` columns anywhere)
- APM tracing, RUM, log management, server-agent metrics
- Kubernetes / cloud-provider scanners

> **The Litmus Test:** *"Would a solo operator or a small-team SRE say 'that's not what I came here for'?"* If yes, it doesn't belong here.
> 
> *Full rationale for these decisions is in [`docs/DESIGN-ORIGINAL.md`](docs/DESIGN-ORIGINAL.md).*

### ✅ In Scope
The core feature set is shipped (29 probe kinds, 130 notification channels, status pages, folders, dependencies, maintenance, 2FA, audit log). What's still welcome:

- **Additional probe kinds** — anything in the spirit of the existing 29. LDAP, AMQP/RabbitMQ, NATS, Cassandra, SNMP v1 GET, mDNS / SSDP, DNS-over-HTTPS, Whois/RDAP HTTP, etc.
- **Additional notification channels** — drop a new adapter into `rampart-notifier/src/channels/` following the pattern of the existing 128 native channels.
- **Importers** — bring monitors in from JSON / CSV / SQLite exports of other self-hosted dashboards. One-shot tools, not background sync.
- **UI polish + bug-fixes** — see open issues tagged [`good-first-issue`](https://github.com/pen-pal/rampart/labels/good-first-issue) and [`help-wanted`](https://github.com/pen-pal/rampart/labels/help-wanted).
- **Docs** — production deployment recipes, Helm charts, Terraform modules, language-specific push-monitor client snippets.

---

## 📂 Project Layout

```text
backend/       Rust workspace, 6 crates (rampart-core / -db / -checker / -scheduler / -notifier / -api)
frontend/      Vite + React SPA, one view per file under src/views/
docs/          Design rationale, architecture, security-debt log
LICENSE        AGPL-3.0-or-later
README.md      Quick start + feature overview
```

**Required Reading Before Touching Code:**
- [`backend/HACKING.md`](backend/HACKING.md) — Crate boundaries, sqlx patterns, probe trait, scheduler design, how to add a new monitor kind end-to-end.
- [`frontend/HACKING.md`](frontend/HACKING.md) — Design tokens, inline-CSS-in-JSX rationale, component patterns.

---

## 🛠️ Setup

See the [README](README.md#run-it-single-binary-recommended) for the canonical setup. Here is the quick version:

```bash
# 1. Start Postgres
cd backend && docker compose up -d postgres
cp .env.example .env

# 2. Frontend bundle (one-shot — only re-run when you change UI)
cd ../frontend && npm install && npm run build

# 3. Run Backend
cd ../backend && cargo run -p rampart-api
```

👉 Open [http://localhost:3000](http://localhost:3000).

**Fast UI Iteration:** Run `npm run dev` in `frontend/` and hit `:5173` — Vite proxies `/v1/*` to `:3000`.

---

## 📏 Conventions & Rules

We have strong opinions to keep the codebase fast and the binary small.

| Rule | Details |
| :--- | :--- |
| 🦀 **Rust** | Edition 2021, **MSRV 1.88** (forced by transitive `time` 0.3.47 / `base64ct` edition2024 deps; the release Dockerfile pins `rust:1.88` to match). |
| ⏱️ **Time** | Use the `time` crate (not `chrono`). Always use `OffsetDateTime` in UTC. |
| 🆔 **IDs** | UUIDv7 wrapped in per-entity newtypes (`MonitorId`, `IncidentId`, etc.). See `rampart-core/src/ids.rs`. **Never** pass a raw `Uuid` between functions when a typed ID exists. |
| 🗄️ **DB Access** | Use `sqlx::query!` / `sqlx::query_as!` (compile-time checked). Raw SQL only, **no query builder DSL**. |
| ❌ **Errors** | Domain errors in `rampart-core::CoreError`, db errors in `rampart-db::DbError`, api errors in `rampart-api::error::ApiError`. **Don't leak `sqlx::Error` to clients.** |
| 🔍 **Probes** | **One probe per file** in `rampart-checker/src/`. Don't unify them prematurely. |
| 🎨 **Frontend CSS** | Inline CSS-in-JSX per view (`<style>{css}</style>`). **No Tailwind, no CSS modules.** Each view is self-contained on purpose. |
| 🤖 **No AI** | Removed in the v2 pivot. **Do not re-introduce AI features.** |

---

## ➕ How to Add a New Monitor Kind

End-to-end, in order:

1. **Migration:** `ALTER TYPE monitor_kind ADD VALUE IF NOT EXISTS 'newthing';` in a new `backend/migrations/000N_*.sql`.
2. **Enum Variant:** Add to `MonitorKind` in `rampart-core/src/monitor.rs`.
3. **Probe:** New file in `rampart-checker/src/newthing.rs` implementing `Probe`. 
   - *Rule:* Always return `Heartbeat`, never `Result`. Failures become heartbeats with `status = Down` and a descriptive `msg`.
4. **Dispatch:** Add the match arm in `rampart-checker/src/lib.rs::Probes::run`.
5. **Wizard UI:** Add to the `types` array in `frontend/src/views/NewMonitorWizard.jsx` with an icon + description + example + placeholder.
6. **Field Requirements:** Update `fieldsFor()` in the same file to declare which inputs this kind needs (`url`, `hostname` + `port`, `dns`, `banner`, etc.).
7. **Port preset (optional):** Add a default to `defaultPort()` if the protocol has a well-known one.
8. **Counts:** Bump the "29 types" line in the wizard intro, the README badge + heading, and `docs/ARCHITECTURE.md`.

*More detail available in [`backend/HACKING.md`](backend/HACKING.md).*

---

## 🧪 Tests & CI

### Backend (Unit + Integration)
~131 tests. `sqlx::test` makes its own per-test isolated databases off the base URL.

```bash
cd backend
docker compose up -d postgres
DATABASE_URL=postgres://rampart:rampart@localhost:5432/rampart sqlx migrate run --source migrations

cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

**⚠️ Crucial: SQLx Offline Cache**
CI builds use `SQLX_OFFLINE=true` against the committed `.sqlx/` cache. If you change a `sqlx::query!` (including adding a migration that a query depends on), you **must** regenerate it and commit the result, or CI will fail:

```bash
cd backend
DATABASE_URL=postgres://rampart:rampart@localhost:5432/rampart cargo sqlx prepare --workspace
git add .sqlx
```

**Security Scanning (`cargo-deny`)**
CI runs a Rust dependency security gate. Reproduce locally before pushing:
```bash
cd backend
cargo install cargo-deny   # one-time
cargo deny check           # advisories (RUSTSEC) + license policy + bans + sources
```
*Policy lives in [`backend/deny.toml`](backend/deny.toml). The project is AGPL-3.0, so new dependencies must be on the allow-list. If `cargo deny` rejects a transitive crate with a legitimate license, add it to the allow-list in the same PR.*

### Frontend (Unit)
~32 tests using Vitest.

```bash
cd frontend
npm ci                 # one-time
npm test               # vitest run
npm run build          # vite build, surfaces JSX errors
```

### End-to-End (Playwright)
17 flows × 5 browser projects (Chromium, Firefox, WebKit + branded Chrome / Edge channels) = 85 cross-browser runs per CI push.

```bash
cd backend && cargo build -p rampart-api      # one-time + when api changes
cd frontend
npm ci && npm run build                       # one-time + after UI changes
npx playwright install                        # bundled engines (chromium / firefox / webkit)
npx playwright install chrome msedge          # optional — branded channels
npx playwright test                           # all available browsers
npx playwright test --project=chromium        # just chromium (fastest)
npx playwright test --ui                      # interactive debugger
```
*Note: E2E spins up a dedicated `rampart_test` database, runs migrations, and launches `rampart-api` on port 3001 — it won't fight your dev `:3000` process. Brave / Vivaldi / Arc are Chromium forks covered by `chromium` + `chrome`; LibreWolf is a Firefox fork covered by `firefox`.*

The full CI gate runs all of the above on push + PR — see [`.github/workflows/ci.yml`](.github/workflows/ci.yml).

---

## 📬 Pull Requests

- **Branch from `main`.** Name it `kind/short-description` (e.g., `probe/dns`, `fix/scheduler-leak`, `docs/sqlx-cache`).
- **One concern per PR.** A feature and a refactor must go in separate PRs.
- **Commit messages:** Short imperative subject (≤70 chars), body explains the *why*. Reference issues like `Fixes #42` when applicable.
- **No `--no-verify`.** If a git hook fails, fix the underlying issue.
- **Don't bump versions** in PRs — that's done at release time.

---

## 🐛 Reporting Bugs / Requesting Features

Before opening an issue:

1. **Check the scope:** Ensure it doesn't fall into the [Out of Scope](#-out-of-scope) list above — those PRs get closed as `wontfix`.
2. **Search existing alerts:** The Dependabot + CodeQL panels under [Security](https://github.com/pen-pal/rampart/security) already track most known dependency / static-analysis issues with the accepted ones documented in [`docs/SECURITY-DEBT.md`](docs/SECURITY-DEBT.md).
3. **For bugs:** Include `rampart-api` version (`/healthz` exposes it), Postgres version, browser + OS (for UI bugs), relevant log lines, and steps to reproduce. *(Tip: log lines from `rampart_scheduler` are usually the most informative.)*
4. **For features:** Explain the use case in concrete terms.
   - ❌ *"Some other tool has X"* is not a use case.
   - ✅ *"I run a homelab and need to be told when my Plex server stops responding"* is a great use case.
5. **For vulnerabilities:** Do **not** open a public issue. Use GitHub's [private vulnerability reporting](https://github.com/pen-pal/rampart/security/advisories/new) so we can ship a patch before it's on the issue tracker.

---

## ⚖️ License

By contributing, you agree your contributions will be licensed under [**AGPL-3.0-or-later**](LICENSE), the same license as the project. 

There is no CLA. We use AGPL deliberately to keep the project and its derivatives in the open. If you can't accept that, this isn't the project for you, and that's completely fine.
