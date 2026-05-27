//! Database layer.
//!
//! Thin repository functions over sqlx. Raw SQL with sqlx's compile-time
//! check; no query builder DSL.
//!
//! Single-tenant by design: no workspace_id scoping anywhere. AuthN/Z
//! happens at the API layer.

pub mod api_keys;
pub mod audit;
pub mod heartbeats;
pub mod incidents;
pub mod maintenance;
pub mod monitors;
pub mod notifications;
pub mod proxies;
pub mod prune;
pub mod recovery_codes;
pub mod sessions;
pub mod settings;
pub mod subscribers;
pub mod status_pages;
pub mod tags;
pub mod templates;
pub mod users;

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
pub async fn connect(database_url: &str, max_connections: u32) -> DbResult<DbPool> {
    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(5))
        .idle_timeout(Some(Duration::from_secs(600)))
        .max_lifetime(Some(Duration::from_secs(1800)))
        .connect(database_url)
        .await?;
    Ok(pool)
}

/// Run all pending migrations from the workspace `migrations/` directory.
/// Idempotent — safe to call on every boot.
pub async fn migrate(pool: &DbPool) -> DbResult<()> {
    sqlx::migrate!("../../migrations").run(pool).await?;
    Ok(())
}
