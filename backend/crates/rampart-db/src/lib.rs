//! Database layer.
//!
//! Thin repository functions over sqlx. Raw SQL with sqlx's compile-time
//! check; no query builder DSL.
//!
//! Multi-tenant (org-scoped): tenant-root tables carry an `org_id` and the
//! repository fns take an `OrgId` to scope reads/writes; system callers use the
//! explicit `*_all`/`*_unscoped` siblings. AuthN/Z happens at the API layer
//! (which resolves the `OrgContext` and passes the org down). See
//! `docs/MULTITENANCY.md`.

pub mod access_review;
pub mod agents;
pub mod api_keys;
pub mod audit;
pub mod delivery_log;
pub mod deploy_markers;
pub mod detection;
pub mod digest_buffer;
pub mod error_tracking;
pub mod escalations;
pub mod heartbeats;
pub mod incident_templates;
pub mod incidents;
pub mod ingest_keys;
pub mod ingest_tokens;
pub mod leader;
pub mod logs;
pub mod maintenance;
pub mod metric_rules;
pub mod metric_samples;
pub mod metrics;
pub mod monitor_groups;
pub mod monitor_presets;
pub mod monitor_templates;
pub mod monitors;
pub mod notifications;
pub mod oidc_state;
pub mod on_call;
pub mod orgs;
pub mod profiles;
pub mod proxies;
pub mod prune;
pub mod rate_limit;
pub mod recovery_codes;
pub mod rls;
pub mod routing;
pub mod rum;
pub mod scheduled_reports;
pub mod secrets;
pub mod sessions;
pub mod settings;
#[cfg(feature = "sqlite")]
pub mod sqlite;
pub mod silences;
pub mod slos;
pub mod source_maps;
pub mod status_pages;
pub mod store;
pub mod subscribers;
pub mod tags;
pub mod telemetry_rules;
pub mod templates;
pub mod traces;
pub mod users;
pub mod webpush;

// Re-export the test fixture helpers from rampart-core so integration
// tests in this crate's tests/ dir can pull them via `rampart_db::testing`.
#[cfg(any(test, feature = "testing"))]
pub use rampart_core::testing;

use sqlx::postgres::{PgPool, PgPoolOptions};
use std::time::Duration;
use thiserror::Error;

pub type DbPool = PgPool;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("database: {0}")]
    Sqlx(#[from] sqlx::Error),

    #[error("migration: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),

    #[error("not found")]
    NotFound,

    #[error("conflict: {0}")]
    Conflict(String),
}

pub type DbResult<T> = Result<T, DbError>;

/// Build a Postgres pool tuned for a single-instance install.
///
/// Defaults to 16 connections — fine for a homelab. Raise via env for
/// production. `acquire_timeout` 5s so DB pressure surfaces as 503s
/// quickly rather than queueing requests until the user gives up.
///
/// ## Row-Level Security (defense-in-depth, `RAMPART_RLS`)
///
/// When `RAMPART_RLS` is set (default OFF), the pool installs `before_acquire`
/// and `after_release` hooks that bind the per-request tenant org (read from
/// the [`rls::CURRENT_ORG`] task-local) onto each checked-out connection.
///
/// With an org bound the hook issues `SET ROLE rampart_app` plus
/// `set_config('app.current_org', …)` so the dormant org-isolation policies
/// (migration 0115, enabled only by an operator) scope every statement to that
/// org. With no org bound (system loops) it issues `RESET ROLE` and clears the
/// GUC, so the connection runs as the BYPASSRLS owner and is unaffected.
///
/// The branch happens ONCE here at construction — flag-off installs NO hooks at
/// all (the plain `.connect()` path), so a flag-off pool is byte-identical to
/// before this feature existed.
pub async fn connect(database_url: &str, max_connections: u32) -> DbResult<DbPool> {
    let opts = PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(5))
        .idle_timeout(Some(Duration::from_secs(600)))
        .max_lifetime(Some(Duration::from_secs(1800)));

    let opts = if rls::rls_enabled() {
        opts.before_acquire(|conn, _meta| Box::pin(bind_rls_org(conn)))
            .after_release(|conn, _meta| Box::pin(reset_rls_org(conn)))
    } else {
        opts
    };

    let pool = opts.connect(database_url).await?;
    Ok(pool)
}

/// `before_acquire` hook: bind the current request's tenant org onto `conn`
/// just before it is handed out, reading the [`rls::CURRENT_ORG`] task-local.
///
/// All-or-nothing + fail-loud: every statement runs in ONE batched simple
/// query, so a partial apply is impossible, and any error propagates as `Err`
/// (NEVER `Ok(false)`, which would silently recycle a possibly mis-bound
/// connection). The org is a validated `Uuid`, so inlining it into the GUC
/// literal is injection-safe.
///
/// We always `RESET ROLE` first so a connection that was previously bound to
/// some other org (and is now being reused) can't leak the prior role/GUC: the
/// reset returns it to the owner, then we set the new GUC, then assume the
/// `rampart_app` role. Ordering GUC-before-role means the role switch can't
/// strand the connection as `rampart_app` carrying a stale org.
async fn bind_rls_org(conn: &mut sqlx::postgres::PgConnection) -> Result<bool, sqlx::Error> {
    use sqlx::Executor;
    let bound = rls::CURRENT_ORG.try_with(|o| *o).ok().flatten();
    let sql = match bound {
        Some(org) => format!(
            "RESET ROLE; SELECT set_config('app.current_org', '{org}', false); SET ROLE rampart_app;"
        ),
        None => {
            // No tenant bound (system loop): owner role, empty GUC.
            "RESET ROLE; SELECT set_config('app.current_org', '', false);".to_string()
        }
    };
    // `AssertSqlSafe`: the only interpolation is `org`, a validated `Uuid`
    // (hyphenated hex only — no quotes/semicolons), so this is injection-safe.
    conn.execute(sqlx::raw_sql(sqlx::AssertSqlSafe(sql)))
        .await?;
    Ok(true)
}

/// `after_release` hook: scrub the per-request binding so a returned connection
/// goes back to the idle pool as the plain owner with an empty GUC. Fail-loud
/// (`Err` not `Ok(false)`) — a connection we can't reset is dropped, not reused.
async fn reset_rls_org(conn: &mut sqlx::postgres::PgConnection) -> Result<bool, sqlx::Error> {
    use sqlx::Executor;
    conn.execute(sqlx::raw_sql(
        "RESET ROLE; SELECT set_config('app.current_org', '', false);",
    ))
    .await?;
    Ok(true)
}

/// Run all pending migrations from the workspace `migrations/` directory.
/// Idempotent — safe to call on every boot.
pub async fn migrate(pool: &DbPool) -> DbResult<()> {
    sqlx::migrate!("../../migrations").run(pool).await?;
    Ok(())
}
