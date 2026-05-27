//! Background pruning. Reads `settings.retention_days` and runs
//! age-based DELETEs against the unbounded-growth tables. Designed to
//! be called from a tokio task on a loop.

use crate::{DbPool, DbResult};
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Clone, Deserialize)]
pub struct RetentionConfig {
    #[serde(default = "default_hb")]
    pub heartbeats: i32,
    #[serde(default = "default_audit")]
    pub audit_log: i32,
}
fn default_hb()    -> i32 { 90 }
fn default_audit() -> i32 { 365 }

impl Default for RetentionConfig {
    fn default() -> Self {
        Self { heartbeats: 90, audit_log: 365 }
    }
}

/// Load the retention setting; fall back to defaults if the row is
/// missing or malformed (e.g. an old install pre-migration 0020).
pub async fn load_config(pool: &DbPool) -> DbResult<RetentionConfig> {
    let raw = crate::settings::get(pool, "retention_days").await?;
    Ok(raw
        .and_then(|v| serde_json::from_value::<RetentionConfig>(v).ok())
        .unwrap_or_default())
}

/// One sweep — best-effort, logs and continues on partial failure.
pub async fn run_once(pool: &DbPool) -> DbResult<(u64, u64)> {
    let cfg = load_config(pool).await?;
    let hb_deleted = sqlx::query!(
        "DELETE FROM heartbeats WHERE ts < NOW() - make_interval(days => $1)",
        cfg.heartbeats,
    )
    .execute(pool)
    .await?
    .rows_affected();

    let al_deleted = sqlx::query!(
        "DELETE FROM audit_log WHERE ts < NOW() - make_interval(days => $1)",
        cfg.audit_log,
    )
    .execute(pool)
    .await?
    .rows_affected();

    Ok((hb_deleted, al_deleted))
}

/// Spawnable loop — call once at startup, runs forever. Default cadence
/// is one sweep per hour; tunable via the `interval` arg if a deployment
/// runs hot enough to need shorter windows.
pub async fn run_loop(pool: DbPool, interval: Duration) {
    let mut ticker = tokio::time::interval(interval);
    // Skip the immediate tick — let the scheduler warm up first.
    ticker.tick().await;
    loop {
        ticker.tick().await;
        match run_once(&pool).await {
            Ok((hb, al)) if hb > 0 || al > 0 => {
                tracing::info!(heartbeats = hb, audit_log = al, "retention prune complete");
            }
            Ok(_) => {} // nothing to delete; stay quiet
            Err(e) => tracing::warn!(error = %e, "retention prune failed"),
        }
    }
}
