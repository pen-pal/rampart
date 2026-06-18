//! Maintenance-window repository.
//!
//! Single-tenant, no workspace scoping — auth happens at the API layer.

use crate::{DbError, DbPool, DbResult};
use rampart_core::maintenance::{
    MaintenanceWindow, NewMaintenanceWindow, Recurrence, UpdateMaintenanceWindow,
};
use rampart_core::status_page::PublicMaintenance;
use rampart_core::{MaintenanceId, MonitorId, StatusPageId};
use time::OffsetDateTime;
use uuid::Uuid;

struct WindowRow {
    id: Uuid,
    name: String,
    description: Option<String>,
    start_at: OffsetDateTime,
    end_at: OffsetDateTime,
    active: bool,
    created_at: OffsetDateTime,
    recurrence: serde_json::Value,
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
            id: MaintenanceId::from_uuid(r.id),
            name: r.name,
            description: r.description,
            start_at: r.start_at,
            end_at: r.end_at,
            active: r.active,
            created_at: r.created_at,
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

pub async fn create(
    pool: &DbPool,
    input: NewMaintenanceWindow,
    org_id: rampart_core::ids::OrgId,
) -> DbResult<MaintenanceWindow> {
    let id = MaintenanceId::new();
    let mut tx = pool.begin().await?;

    let recurrence_json = serde_json::to_value(&input.recurrence)
        .map_err(|e| DbError::Conflict(format!("serialize recurrence: {e}")))?;
    sqlx::query!(
        r#"
        INSERT INTO maintenance_windows
            (id, name, description, start_at, end_at, active, recurrence, org_id)
        VALUES ($1, $2, $3, $4, $5, TRUE, $6, $7)
        "#,
        id.0,
        input.name,
        input.description,
        input.start_at,
        input.end_at,
        recurrence_json,
        org_id.0,
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

pub async fn update(
    pool: &DbPool,
    id: MaintenanceId,
    patch: UpdateMaintenanceWindow,
) -> DbResult<MaintenanceWindow> {
    // Description uses double-Option: outer None = field absent; Some(None)
    // = explicit null (clear); Some(Some(x)) = set. The `description` column
    // is nullable so we map both Some shapes through a single binding.
    let desc_provided = patch.description.is_some();
    let desc_value: Option<String> = patch.description.flatten();
    let recurrence_json = match &patch.recurrence {
        Some(r) => Some(
            serde_json::to_value(r)
                .map_err(|e| DbError::Conflict(format!("serialize recurrence: {e}")))?,
        ),
        None => None,
    };

    let result = sqlx::query!(
        r#"
        UPDATE maintenance_windows
           SET name        = COALESCE($2, name),
               description = CASE WHEN $3::BOOL THEN $4 ELSE description END,
               start_at    = COALESCE($5, start_at),
               end_at      = COALESCE($6, end_at),
               recurrence  = COALESCE($7, recurrence)
         WHERE id = $1
        "#,
        id.0,
        patch.name,
        desc_provided,
        desc_value,
        patch.start_at,
        patch.end_at,
        recurrence_json,
    )
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
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

/// Windows whose active span just changed across `now`, with their
/// monitor sets — driver for the scheduler's maintenance-notification
/// scan. Returns every `active` window that EITHER (a) is live right now
/// but hasn't sent a start notification yet, OR (b) has already left its
/// active span but hasn't sent an end notification yet. The scheduler
/// decides which transition fired and stamps the matching column.
///
/// Recurrence is evaluated in Rust against `now` (same rationale as
/// `is_in_active_window` — the row count is small). The de-dup columns
/// (`notified_start_at` / `notified_end_at`) keep each transition to a
/// single fire even though the scan re-runs every slow tick.
#[derive(Debug, Clone)]
pub struct MaintenanceTransition {
    pub id: MaintenanceId,
    pub name: String,
    pub monitor_ids: Vec<MonitorId>,
    /// True iff the window contains `now` per its recurrence.
    pub currently_active: bool,
    pub notified_start_at: Option<OffsetDateTime>,
    pub notified_end_at: Option<OffsetDateTime>,
}

/// Scan all active windows and return those needing a start- or
/// end-transition notification, hydrated with their monitor sets.
pub async fn transitions_needing_notification(
    pool: &DbPool,
) -> DbResult<Vec<MaintenanceTransition>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, name, start_at, end_at, recurrence,
               notified_start_at, notified_end_at
        FROM maintenance_windows
        WHERE active
        "#,
    )
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        return Ok(Vec::new());
    }

    let now = OffsetDateTime::now_utc();
    let mut out: Vec<MaintenanceTransition> = Vec::new();
    let mut ids: Vec<Uuid> = Vec::new();
    for r in rows {
        let rec: Recurrence = serde_json::from_value(r.recurrence).unwrap_or(Recurrence::None);
        let currently_active = rec.contains(r.start_at, r.end_at, now);
        // Candidate when a transition is still un-notified:
        //   active + no start mark  → start fired this scan
        //   inactive + start mark + no end mark → end fired this scan
        let start_pending = currently_active && r.notified_start_at.is_none();
        let end_pending =
            !currently_active && r.notified_start_at.is_some() && r.notified_end_at.is_none();
        if !start_pending && !end_pending {
            continue;
        }
        ids.push(r.id);
        out.push(MaintenanceTransition {
            id: MaintenanceId::from_uuid(r.id),
            name: r.name,
            monitor_ids: Vec::new(),
            currently_active,
            notified_start_at: r.notified_start_at,
            notified_end_at: r.notified_end_at,
        });
    }

    if out.is_empty() {
        return Ok(out);
    }

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
    for e in edges {
        if let Some(t) = out.iter_mut().find(|t| t.id.0 == e.window_id) {
            t.monitor_ids.push(MonitorId::from_uuid(e.monitor_id));
        }
    }
    Ok(out)
}

/// Stamp `notified_start_at = NOW()` so the start notification fires once.
pub async fn mark_notified_start(pool: &DbPool, id: MaintenanceId) -> DbResult<()> {
    sqlx::query!(
        "UPDATE maintenance_windows SET notified_start_at = NOW() WHERE id = $1",
        id.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Stamp `notified_end_at = NOW()` so the end notification fires once.
pub async fn mark_notified_end(pool: &DbPool, id: MaintenanceId) -> DbResult<()> {
    sqlx::query!(
        "UPDATE maintenance_windows SET notified_end_at = NOW() WHERE id = $1",
        id.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Confirmed status-page email subscribers reachable from any of the
/// given monitors. A monitor reaches a subscriber when both sit on the
/// same status page. DISTINCT so a subscriber on a page hosting several
/// of the monitors is only emailed once. Empty input short-circuits.
pub async fn confirmed_subscriber_emails_for_monitors(
    pool: &DbPool,
    monitors: &[MonitorId],
) -> DbResult<Vec<String>> {
    if monitors.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<Uuid> = monitors.iter().map(|m| m.0).collect();
    let rows = sqlx::query!(
        r#"
        SELECT DISTINCT s.destination
        FROM status_page_subscribers s
        JOIN status_page_monitors spm ON spm.page_id = s.status_page_id
        WHERE spm.monitor_id = ANY($1)
          AND s.channel = 'email'
          AND s.confirmed
        "#,
        &ids,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.destination).collect())
}

/// Active + upcoming maintenance for a status page's public banner.
///
/// Returns the public projection of every `active` window whose monitor
/// set intersects `page`'s monitors and which is EITHER live right now OR
/// has its next occurrence start within `now ..= now + 7 days`. Recurrence
/// is evaluated in Rust (same rationale as `is_in_active_window` — the row
/// count is small). Active windows sort first; within each group, by the
/// occurrence start. `ends_at` is `None` for recurring windows since they
/// have no single end instant.
pub async fn public_for_status_page(
    pool: &DbPool,
    page: StatusPageId,
) -> DbResult<Vec<PublicMaintenance>> {
    // DISTINCT so a window attached to several of the page's monitors
    // only surfaces once. The inner join through both edge tables limits
    // us to windows whose monitor set intersects this page's.
    let rows = sqlx::query!(
        r#"
        SELECT DISTINCT w.id, w.name, w.description, w.start_at, w.end_at, w.recurrence
        FROM maintenance_windows w
        JOIN maintenance_window_monitors mwm ON mwm.window_id = w.id
        JOIN status_page_monitors spm ON spm.monitor_id = mwm.monitor_id
        WHERE spm.page_id = $1
          AND w.active
        "#,
        page.0,
    )
    .fetch_all(pool)
    .await?;

    let now = OffsetDateTime::now_utc();
    let horizon = now.saturating_add(time::Duration::days(7));

    let mut out: Vec<(bool, OffsetDateTime, PublicMaintenance)> = Vec::new();
    for r in rows {
        let rec: Recurrence = serde_json::from_value(r.recurrence).unwrap_or(Recurrence::None);
        let active = rec.contains(r.start_at, r.end_at, now);
        // Resolve the occurrence start we want to surface: the live one
        // if active, otherwise the next upcoming start.
        let starts_at = match rec.next_start(r.start_at, r.end_at, now) {
            Some(s) => s,
            None => continue,
        };
        // Keep only live windows or ones starting inside the 7-day horizon.
        if !active && starts_at > horizon {
            continue;
        }
        // A single-shot window has a concrete end; recurring windows
        // repeat, so we don't pin a single end instant for them.
        let ends_at = matches!(rec, Recurrence::None).then_some(r.end_at);
        out.push((
            active,
            starts_at,
            PublicMaintenance {
                title: r.name,
                description: r.description,
                starts_at,
                ends_at,
                active,
            },
        ));
    }

    // Active first, then by soonest start.
    out.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    Ok(out.into_iter().map(|(_, _, m)| m).collect())
}
