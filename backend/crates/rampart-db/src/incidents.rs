//! Incident queries — status-page announcements.
//!
//! Drastically simpler than Rampart v1's incident model. No correlation,
//! no severity, no per-event timeline. Just: post a message, optionally
//! post updates, mark resolved.

use crate::{DbError, DbPool, DbResult};
use rampart_core::{Incident, IncidentId, IncidentStyle, StatusPageId, UserId};
use serde::Deserialize;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct NewIncident {
    pub title: String,
    pub content: String,
    #[serde(default = "default_style")]
    pub style: IncidentStyle,
    #[serde(default = "default_pinned")]
    pub pinned: bool,
}

fn default_style() -> IncidentStyle {
    IncidentStyle::Warning
}
fn default_pinned() -> bool {
    true
}

pub async fn create(
    pool: &DbPool,
    page: StatusPageId,
    author: Option<UserId>,
    input: NewIncident,
) -> DbResult<Incident> {
    let id = IncidentId::new();
    let row = sqlx::query!(
        r#"
        INSERT INTO incidents
            (id, status_page_id, title, content, style, pinned, created_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING
            id, status_page_id, title, content,
            style AS "style: IncidentStyle",
            pinned, active, resolved_at, created_at, created_by
        "#,
        id.0,
        page.0,
        input.title,
        input.content,
        input.style as IncidentStyle,
        input.pinned,
        author.map(|u| u.0),
    )
    .fetch_one(pool)
    .await?;

    Ok(Incident {
        id: IncidentId::from_uuid(row.id),
        status_page_id: StatusPageId::from_uuid(row.status_page_id),
        title: row.title,
        content: row.content,
        style: row.style,
        pinned: row.pinned,
        active: row.active,
        resolved_at: row.resolved_at,
        created_at: row.created_at,
        created_by: row.created_by.map(UserId::from_uuid),
    })
}

pub async fn list_active(pool: &DbPool, page: StatusPageId) -> DbResult<Vec<Incident>> {
    let rows = sqlx::query!(
        r#"
        SELECT
            id, status_page_id, title, content,
            style AS "style: IncidentStyle",
            pinned, active, resolved_at, created_at, created_by
        FROM incidents
        WHERE status_page_id = $1 AND active
        ORDER BY pinned DESC, created_at DESC
        "#,
        page.0,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| Incident {
            id: IncidentId::from_uuid(r.id),
            status_page_id: StatusPageId::from_uuid(r.status_page_id),
            title: r.title,
            content: r.content,
            style: r.style,
            pinned: r.pinned,
            active: r.active,
            resolved_at: r.resolved_at,
            created_at: r.created_at,
            created_by: r.created_by.map(UserId::from_uuid),
        })
        .collect())
}

pub async fn resolve(pool: &DbPool, id: IncidentId, now: OffsetDateTime) -> DbResult<()> {
    let result = sqlx::query!(
        r#"
        UPDATE incidents
           SET active      = FALSE,
               resolved_at = COALESCE(resolved_at, $2)
         WHERE id = $1
        "#,
        id.0,
        now,
    )
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

/// Append a running update to an open incident.
pub async fn post_update(
    pool: &DbPool,
    incident: IncidentId,
    author: Option<UserId>,
    message: String,
) -> DbResult<Uuid> {
    let id = Uuid::now_v7();
    sqlx::query!(
        r#"
        INSERT INTO incident_updates (id, incident_id, message, posted_by)
        VALUES ($1, $2, $3, $4)
        "#,
        id,
        incident.0,
        message,
        author.map(|u| u.0),
    )
    .execute(pool)
    .await?;
    Ok(id)
}
