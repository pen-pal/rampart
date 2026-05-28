//! Maintenance-window repository.
//!
//! Single-tenant, no workspace scoping — auth happens at the API layer.

use crate::{DbError, DbPool, DbResult};
use rampart_core::maintenance::{MaintenanceWindow, NewMaintenanceWindow, Recurrence};
use rampart_core::{MaintenanceId, MonitorId};
use time::OffsetDateTime;
use uuid::Uuid;

struct WindowRow {
    id:          Uuid,
    name:        String,
    description: Option<String>,
    start_at:    OffsetDateTime,
    end_at:      OffsetDateTime,
    active:      bool,
    created_at:  OffsetDateTime,
    recurrence:  serde_json::Value,
}

impl From<WindowRow> for MaintenanceWindow {
    fn from(r: WindowRow) -> Self {
        // A malformed recurrence blob shouldn't break the whole list —
        // fall back to None and log via tracing. Callers can still see
        // start_at/end_at and resolve the row manually.
        let recurrence = serde_json::from_value(r.recurrence).unwrap_or_else(|e| {
            tracing::warn!(window = %r.id, error = %e, "bad recurrence json — treating as none");
            Recurrence::None
        });
        MaintenanceWindow {
            id:          MaintenanceId::from_uuid(r.id),
            name:        r.name,
            description: r.description,
            start_at:    r.start_at,
            end_at:      r.end_at,
            active:      r.active,
            created_at:  r.created_at,
            monitor_ids: Vec::new(),
            recurrence,
        }
    }
}

pub async fn list(pool: &DbPool) -> DbResult<Vec<MaintenanceWindow>> {
    // Two queries instead of a single GROUP BY: keeps the join simple
    // and lets us hydrate monitor_ids without pulling every monitor row.
    let rows = sqlx::query_as!(
        WindowRow,
        r#"
        SELECT id, name, description, start_at, end_at, active, created_at, recurrence
        FROM maintenance_windows
        ORDER BY start_at DESC
        "#,
    )
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        return Ok(Vec::new());
    }

    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let edges = sqlx::query!(
        r#"
        SELECT window_id, monitor_id
        FROM maintenance_window_monitors
        WHERE window_id = ANY($1)
        "#,
        &ids,
    )
    .fetch_all(pool)
    .await?;

    let mut windows: Vec<MaintenanceWindow> = rows.into_iter().map(Into::into).collect();
    for e in edges {
        if let Some(w) = windows.iter_mut().find(|w| w.id.0 == e.window_id) {
            w.monitor_ids.push(MonitorId::from_uuid(e.monitor_id));
        }
    }
    Ok(windows)
}

pub async fn get(pool: &DbPool, id: MaintenanceId) -> DbResult<MaintenanceWindow> {
    let row = sqlx::query_as!(
        WindowRow,
        r#"
        SELECT id, name, description, start_at, end_at, active, created_at, recurrence
        FROM maintenance_windows
        WHERE id = $1
        "#,
        id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;

    let edges = sqlx::query!(
        r#"SELECT monitor_id FROM maintenance_window_monitors WHERE window_id = $1"#,
        id.0,
    )
    .fetch_all(pool)
    .await?;

    let mut w: MaintenanceWindow = row.into();
    w.monitor_ids = edges
        .into_iter()
        .map(|e| MonitorId::from_uuid(e.monitor_id))
        .collect();
    Ok(w)
}

pub async fn create(pool: &DbPool, input: NewMaintenanceWindow) -> DbResult<MaintenanceWindow> {
    let id = MaintenanceId::new();
    let mut tx = pool.begin().await?;

    let recurrence_json = serde_json::to_value(&input.recurrence).map_err(|e| {
        DbError::Conflict(format!("serialize recurrence: {e}"))
    })?;
    sqlx::query!(
        r#"
        INSERT INTO maintenance_windows
            (id, name, description, start_at, end_at, active, recurrence)
        VALUES ($1, $2, $3, $4, $5, TRUE, $6)
        "#,
        id.0,
        input.name,
        input.description,
        input.start_at,
        input.end_at,
        recurrence_json,
    )
    .execute(&mut *tx)
    .await?;

    for mid in &input.monitor_ids {
        sqlx::query!(
            r#"INSERT INTO maintenance_window_monitors (window_id, monitor_id) VALUES ($1, $2)
               ON CONFLICT DO NOTHING"#,
            id.0,
            mid.0,
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    get(pool, id).await
}

pub async fn delete(pool: &DbPool, id: MaintenanceId) -> DbResult<()> {
    let result = sqlx::query!("DELETE FROM maintenance_windows WHERE id = $1", id.0)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

pub async fn set_active(pool: &DbPool, id: MaintenanceId, active: bool) -> DbResult<()> {
    let result = sqlx::query!(
        "UPDATE maintenance_windows SET active = $1 WHERE id = $2",
        active,
        id.0,
    )
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

pub async fn attach(pool: &DbPool, window: MaintenanceId, monitor: MonitorId) -> DbResult<()> {
    sqlx::query!(
        r#"INSERT INTO maintenance_window_monitors (window_id, monitor_id) VALUES ($1, $2)
           ON CONFLICT DO NOTHING"#,
        window.0,
        monitor.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn detach(pool: &DbPool, window: MaintenanceId, monitor: MonitorId) -> DbResult<()> {
    sqlx::query!(
        r#"DELETE FROM maintenance_window_monitors WHERE window_id = $1 AND monitor_id = $2"#,
        window.0,
        monitor.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Returns true iff `monitor` is currently covered by any active window
/// that contains NOW(). Recurrence eval runs in Rust against the
/// monitor's attached windows — the row count is small (windows are
/// user-created, typically tens, not thousands) so a scan + serde decode
/// per tick is cheaper than maintaining a JSONB-aware partial index.
pub async fn is_in_active_window(pool: &DbPool, monitor: MonitorId) -> DbResult<bool> {
    let rows = sqlx::query!(
        r#"
        SELECT w.start_at, w.end_at, w.recurrence
        FROM maintenance_windows w
        JOIN maintenance_window_monitors m ON m.window_id = w.id
        WHERE m.monitor_id = $1
          AND w.active
        "#,
        monitor.0,
    )
    .fetch_all(pool)
    .await?;

    let now = OffsetDateTime::now_utc();
    for r in rows {
        let rec: Recurrence = serde_json::from_value(r.recurrence).unwrap_or(Recurrence::None);
        if rec.contains(r.start_at, r.end_at, now) {
            return Ok(true);
        }
    }
    Ok(false)
}
