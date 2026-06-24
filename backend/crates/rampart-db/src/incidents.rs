//! Incident queries — status-page announcements.
//!
//! Drastically simpler than Rampart v1's incident model. No correlation,
//! no severity, no per-event timeline. Just: post a message, optionally
//! post updates, mark resolved.

use crate::{DbError, DbPool, DbResult};
use rampart_core::ids::OrgId;
use rampart_core::{
    Incident, IncidentId, IncidentStyle, IncidentUpdate, IncidentUpdateId, StatusPageId, UserId,
};
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
    /// Stable de-duplication key for webhook-driven ingest. `None` for
    /// manually authored incidents.
    #[serde(default)]
    pub dedup_key: Option<String>,
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
            (id, status_page_id, title, content, style, pinned, created_by, dedup_key)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        RETURNING
            id, status_page_id, title, content,
            style AS "style: IncidentStyle",
            pinned, active, resolved_at, created_at, created_by, dedup_key
        "#,
        id.0,
        page.0,
        input.title,
        input.content,
        input.style as IncidentStyle,
        input.pinned,
        author.map(|u| u.0),
        input.dedup_key,
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
        dedup_key: row.dedup_key,
    })
}

/// Find the active incident on `page` whose `dedup_key` matches `key`.
///
/// Webhook ingest uses this to resolve a firing incident exactly: the
/// vendor "resolved/recovered" payload carries the same stable key it sent
/// on firing, so we match on that instead of the fragile title. The partial
/// unique index on (status_page_id, dedup_key) WHERE active guarantees at
/// most one row here.
pub async fn find_active_by_dedup_key(
    pool: &DbPool,
    page: StatusPageId,
    key: &str,
) -> DbResult<Option<Incident>> {
    let row = sqlx::query!(
        r#"
        SELECT
            id, status_page_id, title, content,
            style AS "style: IncidentStyle",
            pinned, active, resolved_at, created_at, created_by, dedup_key
        FROM incidents
        WHERE status_page_id = $1 AND active AND dedup_key = $2
        ORDER BY created_at DESC
        LIMIT 1
        "#,
        page.0,
        key,
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| Incident {
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
        dedup_key: r.dedup_key,
    }))
}

pub async fn list_active(pool: &DbPool, page: StatusPageId) -> DbResult<Vec<Incident>> {
    let rows = sqlx::query!(
        r#"
        SELECT
            id, status_page_id, title, content,
            style AS "style: IncidentStyle",
            pinned, active, resolved_at, created_at, created_by, dedup_key
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
            dedup_key: r.dedup_key,
        })
        .collect())
}

/// Recent incidents across every status page — the dashboard's "Recent
/// incidents" feed (active ones first, then newest). Capped at `limit`.
pub async fn recent(pool: &DbPool, limit: i64, org_id: OrgId) -> DbResult<Vec<Incident>> {
    let rows = sqlx::query!(
        r#"
        SELECT
            i.id, i.status_page_id, i.title, i.content,
            i.style AS "style: IncidentStyle",
            i.pinned, i.active, i.resolved_at, i.created_at, i.created_by, i.dedup_key
        FROM incidents i
        JOIN status_pages sp ON sp.id = i.status_page_id
        WHERE sp.org_id = $2
        ORDER BY i.active DESC, i.created_at DESC
        LIMIT $1
        "#,
        limit.clamp(1, 50),
        org_id.0,
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
            dedup_key: r.dedup_key,
        })
        .collect())
}

/// Resolved incidents for the public history pane, newest-first. Capped
/// at `limit` so a long-lived page can't dump its full history into one
/// page response. The public-view shape only needs the title / content /
/// style / created_at / resolved_at fields, but we share the full
/// `Incident` type with `list_active` to keep the row mapping in one
/// place; the caller projects what it needs.
pub async fn list_resolved_history(
    pool: &DbPool,
    page: StatusPageId,
    limit: i64,
) -> DbResult<Vec<Incident>> {
    let rows = sqlx::query!(
        r#"
        SELECT
            id, status_page_id, title, content,
            style AS "style: IncidentStyle",
            pinned, active, resolved_at, created_at, created_by, dedup_key
        FROM incidents
        WHERE status_page_id = $1 AND NOT active
        ORDER BY COALESCE(resolved_at, created_at) DESC
        LIMIT $2
        "#,
        page.0,
        limit,
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
            dedup_key: r.dedup_key,
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

/// Full history (active and resolved) for a page. Admin side. `limit` is
/// clamped to a sane ceiling so the response can't grow unbounded with age.
pub async fn list_all(pool: &DbPool, page: StatusPageId, limit: i64) -> DbResult<Vec<Incident>> {
    let rows = sqlx::query!(
        r#"
        SELECT
            id, status_page_id, title, content,
            style AS "style: IncidentStyle",
            pinned, active, resolved_at, created_at, created_by, dedup_key
        FROM incidents
        WHERE status_page_id = $1
        ORDER BY active DESC, pinned DESC, created_at DESC
        LIMIT $2
        "#,
        page.0,
        limit.clamp(1, 500),
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
            dedup_key: r.dedup_key,
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
    pub title: Option<String>,
    pub content: Option<String>,
    pub style: Option<IncidentStyle>,
    pub pinned: Option<bool>,
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
               pinned, active, resolved_at, created_at, created_by, dedup_key
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
        dedup_key: row.dedup_key,
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

#[cfg(test)]
mod tests {
    use super::*;
    use rampart_core::org::DEFAULT_ORG_ID;
    use sqlx::PgPool;

    /// CORRECTNESS (audit re-rank #8): a second active incident with the same
    /// (status_page_id, dedup_key) hits the partial unique index and surfaces as
    /// a `DbError::Sqlx` unique violation — the exact shape the webhook ingest
    /// route catches as "already reported" so a concurrent duplicate firing
    /// doesn't 500 and abort the rest of the batch.
    #[sqlx::test(migrations = "../../migrations")]
    async fn duplicate_active_dedup_key_is_unique_violation(pool: PgPool) {
        let org = OrgId::from_uuid(DEFAULT_ORG_ID);
        let page = crate::status_pages::create(
            &pool,
            serde_json::from_value(serde_json::json!({ "slug": "page", "title": "P" })).unwrap(),
            org,
        )
        .await
        .unwrap();
        let mk = || -> NewIncident {
            serde_json::from_value(serde_json::json!({
                "title": "t", "content": "c", "dedup_key": "k"
            }))
            .unwrap()
        };
        create(&pool, page.id, None, mk()).await.unwrap();
        let err = create(&pool, page.id, None, mk()).await.unwrap_err();
        assert!(
            matches!(&err, DbError::Sqlx(e) if e.as_database_error().is_some_and(|d| d.is_unique_violation())),
            "expected a unique violation, got {err:?}"
        );
    }
}
