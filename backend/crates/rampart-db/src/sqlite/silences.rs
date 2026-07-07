//! SQLite `silences` domain — the dispatch-path read `is_silenced` (the single
//! chokepoint every alert flows through). CRUD stays stubbed until a
//! management-API slice needs it. Dialect: ts→INTEGER unix-seconds.

use crate::DbResult;
use sqlx::SqlitePool;
use time::OffsetDateTime;
use uuid::Uuid;

/// Is any active silence covering `monitor` (or a global silence)? Mirrors PG:
/// a silence matches when not expired AND (global OR scoped to this monitor).
pub async fn is_silenced(pool: &SqlitePool, monitor: Option<Uuid>) -> DbResult<bool> {
    let now = OffsetDateTime::now_utc().unix_timestamp();
    // A GLOBAL silence (monitor_id IS NULL) only mutes same-org monitors (see PG).
    let (n,): (i64,) = sqlx::query_as(
        "SELECT EXISTS(
            SELECT 1 FROM silences
            WHERE (expires_at IS NULL OR expires_at > ?)
              AND (
                monitor_id = ?
                OR (monitor_id IS NULL AND (
                     ? IS NULL
                     OR org_id = (SELECT org_id FROM monitors WHERE id = ?)
                ))
              )
        )",
    )
    .bind(now)
    .bind(monitor.map(|m| m.to_string()))
    .bind(monitor.map(|m| m.to_string()))
    .bind(monitor.map(|m| m.to_string()))
    .fetch_one(pool)
    .await?;
    Ok(n != 0)
}
