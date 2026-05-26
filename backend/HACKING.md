# backend/HACKING.md

Rust-specific conventions for the `rampart-*` crates. Read this before touching code in `backend/crates/`. The top-level [`README`](../README.md) covers how to run; [`docs/DESIGN.md`](../docs/DESIGN.md) covers the why and the v1→v2 pivot history.

## Crate boundaries (do not violate)

```
rampart-core      No I/O. Types, IDs, errors. Depended on by every other crate.
rampart-db        sqlx repository. Depends on rampart-core. NEVER imports axum.
rampart-checker   Probe trait + runners. Depends on rampart-core. NEVER imports rampart-db.
rampart-scheduler Owns probe tasks + writer. Depends on -core, -db, -checker.
rampart-api       The binary. Depends on all of the above. Owns auth, routing.
```

If you find yourself wanting `rampart-checker` to read from the DB, you're holding it wrong — the scheduler is the layer that mediates. Probes get a `&Monitor` value, return a `Heartbeat` value.

## sqlx patterns

We use `sqlx::query!` and `sqlx::query_as!` exclusively — they compile-time check SQL against a real database. Two gotchas:

### 1. The database must exist at build time, OR you need an offline cache

```bash
# Online (development): just have Postgres running with migrations applied.
docker compose up -d postgres
cargo run -p rampart-api          # migrates on boot
# now `cargo build` validates queries against the live schema

# Offline (CI):
cargo sqlx prepare --workspace   # generates .sqlx/, commit it
# in CI: SQLX_OFFLINE=true cargo build
```

### 2. Enum array bindings need a text-cast workaround

The custom enum types (`monitor_status`, `check_status`, etc.) don't implement `PgHasArrayType` automatically. For bulk inserts via `UNNEST`, bind the enum array as `text[]` and cast in SQL:

```rust
// in rampart-db/src/heartbeats.rs::insert_many
let mut statuses: Vec<String> = ...;  // bind as text
// SQL:
//   SELECT * FROM UNNEST(..., $3::text[]::monitor_status[], ...)
```

Don't try to bind `Vec<MonitorStatus>` directly. It won't compile, and the error is cryptic.

### 3. Returning enums from query!

Use the `AS "col: EnumType"` syntax in the SELECT, e.g.:

```rust
sqlx::query!(
    r#"SELECT status AS "status: MonitorStatus" FROM heartbeats WHERE ..."#,
    ...
)
```

### 4. NUMERIC columns

We deliberately avoid them. They require the `bigdecimal` or `rust_decimal` sqlx feature. Use `REAL` (f32) or `DOUBLE PRECISION` (f64) instead. There's no NUMERIC column in the current schema; keep it that way.

## The Probe trait

`rampart-checker::Probe` is the central extension point:

```rust
#[async_trait]
pub trait Probe: Send + Sync {
    async fn run(&self, monitor: &Monitor) -> Heartbeat;
}
```

**Always returns `Heartbeat`** — never `Result`. Failures become heartbeats with `status = Down` and a descriptive `msg`. This is so the scheduler never has to handle errors from probes; the heartbeat record IS the error report.

When adding a new probe (e.g. DNS):
1. Create `rampart-checker/src/dns.rs` with `pub struct DnsProbe;` and `impl Probe for DnsProbe`
2. Wire it into the dispatcher in `lib.rs::Probes::run` match arm
3. Read kind-specific config from `monitor.config` (JSONB blob) — see `http.rs::json_path_matches` for the pattern
4. Cap any response body or output size to bound memory (`http.rs` uses 512 KiB)

## Scheduler design

`rampart-scheduler::Scheduler` runs forever. Key invariants:

- **One tokio task per active monitor.** Cheap enough for hundreds; revisit only if a deployment needs thousands.
- **All heartbeats flow through one mpsc channel** to a single writer task that batches by size (256) or wall time (1 second). The channel buffer is 4096; if it fills, probe tasks block, which is the correct backpressure behavior.
- **Status flips are detected inside the probe task** via `Arc<RwLock<MonitorStatus>>`. The `important = true` flag on the heartbeat is the only marker; nothing else needs to scan the series.
- **Reload-on-mutation:** API routes call `state.poke_scheduler()` which `notify_one()`s the reload `Notify`. Fallback timer reconciles every 30 seconds.
- **Cancellation** uses `Notify` rather than `JoinHandle::abort()` — abort risks killing the task mid-flush.

If you change the batching strategy, measure first. The current numbers are reasonable defaults, not magic.

## ID newtypes

In `rampart-core/src/ids.rs`. One macro invocation per entity:

```rust
id!(MonitorId);
```

Generates a `Copy` newtype around `Uuid` with `Serialize`, `Deserialize`, `sqlx::Type`, `Display`, `From<Uuid>`, `Into<Uuid>`. Always use these in function signatures. The compiler will catch mix-ups (passing `IncidentId` where `MonitorId` is expected) before they become runtime bugs.

When pulling rows from sqlx, the raw column is `Uuid`; convert with `MonitorId::from_uuid(r.id)`.

## Adding a new monitor kind end-to-end

This is the most common kind of change. Order matters:

1. **Migration:** `ALTER TYPE monitor_kind ADD VALUE IF NOT EXISTS 'newthing';` in a new `migrations/000N_*.sql`. `ADD VALUE` is additive so it's safe — but read [the PG docs note](https://www.postgresql.org/docs/current/sql-altertype.html) about transaction restrictions.
2. **Enum variant:** add `NewThing,` to `MonitorKind` in `rampart-core/src/monitor.rs` (preserve serde rename rules — they're `snake_case`).
3. **Probe:** `rampart-checker/src/newthing.rs` with `impl Probe`.
4. **Dispatch:** add the match arm in `rampart-checker/src/lib.rs::Probes::run`.
5. **Wizard UI:** add the type to the `types` array in `frontend/src/views/NewMonitorWizard.jsx` with icon + description.
6. **Field requirements:** update `fieldsFor()` in the same file so the form shows the right inputs.

## Adding a new notification channel

1. **Enum variant** in `rampart-core/src/notification.rs::ChannelKind` + matching `ALTER TYPE channel_kind ADD VALUE` migration.
2. In the planned `rampart-notifier` crate (not yet built), one file per channel implementing a `Channel` trait similar to `Probe`. Render the body via the shared template renderer; the per-channel code is just delivery (HTTP POST, SMTP send, etc.).
3. Wizard UI gets a new row in the channel-picker.

## Things that look wrong but are intentional

- `slo_target` is not in the schema. Was removed during the v1→v2 pivot.
- The `incidents` table has no `severity` column — only `style`. Incidents here are status-page announcements, not investigation records.
- There's no `incident_events` or `action_items` table. Removed in pivot.
- `monitors` has no `workspace_id`. Single-tenant.
- `routing_rules` doesn't exist. Use `monitor_notifications` for direct fan-out.

## Common cargo commands

```bash
cargo run -p rampart-api
cargo check --workspace                    # fast type-check, no codegen
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all

# Database
cargo install sqlx-cli --no-default-features --features rustls,postgres
cargo sqlx migrate add some_change          # creates a new migration file
cargo sqlx migrate run                      # apply (the app also does this on boot)
cargo sqlx prepare --workspace              # regenerate .sqlx/ cache
```
