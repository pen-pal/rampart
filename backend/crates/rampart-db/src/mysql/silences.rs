//! MySQL `silences` domain — the dispatch-path read `is_silenced` (the single
//! chokepoint every alert flows through). CRUD stays stubbed on `MysqlStore`
//! until a management-API slice needs it. Dialect: ts→BIGINT unix-seconds.

use crate::DbResult;
use sqlx::MySqlPool;
use time::OffsetDateTime;
use uuid::Uuid;

/// Is any active silence covering `monitor` (or a global silence)? Mirrors PG:
/// a silence matches when not expired AND (global OR scoped to this monitor).
pub async fn is_silenced(pool: &MySqlPool, monitor: Option<Uuid>) -> DbResult<bool> {
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let (n,): (i64,) = sqlx::query_as(
        "SELECT EXISTS(
            SELECT 1 FROM silences
            WHERE (expires_at IS NULL OR expires_at > ?)
              AND (monitor_id IS NULL OR monitor_id = ?)
        )",
    )
    .bind(now)
    .bind(monitor.map(|m| m.to_string()))
    .fetch_one(pool)
    .await?;
    Ok(n != 0)
}
