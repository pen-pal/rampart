//! Incident queries — status-page announcements.
//!
//! Drastically simpler than Rampart v1's incident model. No correlation,
//! no severity, no per-event timeline. Just: post a message, optionally
//! post updates, mark resolved.

use crate::{DbError, DbPool, DbResult};
use rampart_core::{Incident, IncidentId, IncidentStyle, IncidentUpdate, IncidentUpdateId,
    StatusPageId, UserId};
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

/// Full history (active and resolved) for a page. Admin side.
pub async fn list_all(pool: &DbPool, page: StatusPageId) -> DbResult<Vec<Incident>> {
    let rows = sqlx::query!(
        r#"
        SELECT
            id, status_page_id, title, content,
            style AS "style: IncidentStyle",
            pinned, active, resolved_at, created_at, created_by
        FROM incidents
        WHERE status_page_id = $1
        ORDER BY active DESC, pinned DESC, created_at DESC
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

pub async fn delete(pool: &DbPool, id: IncidentId) -> DbResult<()> {
    let result = sqlx::query!("DELETE FROM incidents WHERE id = $1", id.0)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct UpdateIncident {
    pub title:   Option<String>,
    pub content: Option<String>,
    pub style:   Option<IncidentStyle>,
    pub pinned:  Option<bool>,
}

pub async fn update(pool: &DbPool, id: IncidentId, patch: UpdateIncident) -> DbResult<Incident> {
    let result = sqlx::query!(
        r#"
        UPDATE incidents SET
            title   = COALESCE($2, title),
            content = COALESCE($3, content),
            style   = COALESCE($4, style),
            pinned  = COALESCE($5, pinned)
        WHERE id = $1
        "#,
        id.0,
        patch.title,
        patch.content,
        patch.style as Option<IncidentStyle>,
        patch.pinned,
    )
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    get(pool, id).await
}

pub async fn get(pool: &DbPool, id: IncidentId) -> DbResult<Incident> {
    let row = sqlx::query!(
        r#"
        SELECT id, status_page_id, title, content,
               style AS "style: IncidentStyle",
               pinned, active, resolved_at, created_at, created_by
        FROM incidents WHERE id = $1
        "#,
        id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;

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

pub async fn list_updates(pool: &DbPool, incident: IncidentId) -> DbResult<Vec<IncidentUpdate>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, incident_id, message, posted_at, posted_by
        FROM incident_updates
        WHERE incident_id = $1
        ORDER BY posted_at ASC
        "#,
        incident.0,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| IncidentUpdate {
            id: IncidentUpdateId::from_uuid(r.id),
            incident_id: IncidentId::from_uuid(r.incident_id),
            message: r.message,
            posted_at: r.posted_at,
            posted_by: r.posted_by.map(UserId::from_uuid),
        })
        .collect())
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
