//! MySQL `monitor_groups` domain — the dispatch-path read `any_parent_down`
//! (dependency suppression: don't page a service when its upstream is down).
//! The folder/dependency CRUD stays stubbed until a management-API slice needs
//! it; the routing read that consumes the folder tree lives in `mysql::routing`.

use crate::DbResult;
use rampart_core::ids::MonitorId;
use sqlx::MySqlPool;

/// Does `child` depend on any monitor that's currently down/pending? Active
/// parents only (a paused parent doesn't suppress). Mirrors PG/SQLite.
pub async fn any_parent_down(pool: &MySqlPool, child: MonitorId) -> DbResult<bool> {
    let (n,): (i64,) = sqlx::query_as(
        "SELECT EXISTS(
            SELECT 1 FROM monitor_dependencies d
            JOIN monitors p ON p.id = d.depends_on_id
            WHERE d.monitor_id = ? AND p.active = 1
              AND (p.current_status = 'down' OR p.current_status = 'pending')
        )",
    )
    .bind(child.0.to_string())
    .fetch_one(pool)
    .await?;
    Ok(n != 0)
}
