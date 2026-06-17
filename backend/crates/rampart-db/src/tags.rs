//! Tag repository.

use crate::{DbError, DbPool, DbResult};
use rampart_core::ids::OrgId;
use rampart_core::tag::{NewTag, Tag, TagBrief, UpdateTag};
use rampart_core::{MonitorId, TagId};
use serde::Serialize;
use std::collections::HashMap;
use time::OffsetDateTime;
use uuid::Uuid;

struct TagRow {
    id: Uuid,
    name: String,
    color: String,
    created_at: OffsetDateTime,
}

impl From<TagRow> for Tag {
    fn from(r: TagRow) -> Self {
        Tag {
            id: TagId::from_uuid(r.id),
            name: r.name,
            color: r.color,
            created_at: r.created_at,
        }
    }
}

pub async fn list(pool: &DbPool, org_id: OrgId) -> DbResult<Vec<Tag>> {
    let rows = sqlx::query_as!(
        TagRow,
        r#"SELECT id, name, color, created_at FROM tags WHERE org_id = $1 ORDER BY name"#,
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn get(pool: &DbPool, id: TagId, org_id: OrgId) -> DbResult<Tag> {
    let row = sqlx::query_as!(
        TagRow,
        r#"SELECT id, name, color, created_at FROM tags WHERE id = $1 AND org_id = $2"#,
        id.0,
        org_id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(row.into())
}

pub async fn create(pool: &DbPool, input: NewTag) -> DbResult<Tag> {
    let id = TagId::new();
    let row = sqlx::query_as!(
        TagRow,
        r#"
        INSERT INTO tags (id, name, color)
        VALUES ($1, $2, $3)
        RETURNING id, name, color, created_at
        "#,
        id.0,
        input.name,
        input.color,
    )
    .fetch_one(pool)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.code().as_deref() == Some("23505") => {
            DbError::Conflict("tag name is already in use".into())
        }
        _ => DbError::from(e),
    })?;
    Ok(row.into())
}

pub async fn update(pool: &DbPool, id: TagId, patch: UpdateTag, org_id: OrgId) -> DbResult<Tag> {
    let result = sqlx::query!(
        r#"
        UPDATE tags
           SET name  = COALESCE($2, name),
               color = COALESCE($3, color)
         WHERE id = $1 AND org_id = $4
        "#,
        id.0,
        patch.name,
        patch.color,
        org_id.0,
    )
    .execute(pool)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.code().as_deref() == Some("23505") => {
            DbError::Conflict("tag name is already in use".into())
        }
        _ => DbError::from(e),
    })?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    get(pool, id, org_id).await
}

/// Per-tag usage counts: how many monitors, channels, and folders carry
/// each tag. Returned as a single row per tag (zero rows for an unused
/// tag are filled in by the caller if it wants to render them).
#[derive(Debug, Clone, Serialize)]
pub struct TagUsage {
    pub tag_id: TagId,
    pub monitors: i64,
    pub channels: i64,
    pub groups: i64,
}

pub async fn usage(pool: &DbPool, org_id: OrgId) -> DbResult<Vec<TagUsage>> {
    let rows = sqlx::query!(
        r#"
        SELECT t.id AS tag_id,
               (SELECT COUNT(*) FROM monitor_tags      m WHERE m.tag_id = t.id) AS "monitors!: i64",
               (SELECT COUNT(*) FROM notification_tags n WHERE n.tag_id = t.id) AS "channels!: i64",
               (SELECT COUNT(*) FROM group_tags        g WHERE g.tag_id = t.id) AS "groups!: i64"
        FROM tags t
        WHERE t.org_id = $1
        "#,
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| TagUsage {
            tag_id: TagId::from_uuid(r.tag_id),
            monitors: r.monitors,
            channels: r.channels,
            groups: r.groups,
        })
        .collect())
}

pub async fn delete(pool: &DbPool, id: TagId, org_id: OrgId) -> DbResult<()> {
    let result = sqlx::query!(
        "DELETE FROM tags WHERE id = $1 AND org_id = $2",
        id.0,
        org_id.0,
    )
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

pub async fn attach(pool: &DbPool, monitor: MonitorId, tag: TagId) -> DbResult<()> {
    sqlx::query!(
        r#"
        INSERT INTO monitor_tags (monitor_id, tag_id)
        VALUES ($1, $2)
        ON CONFLICT DO NOTHING
        "#,
        monitor.0,
        tag.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn detach(pool: &DbPool, monitor: MonitorId, tag: TagId) -> DbResult<()> {
    sqlx::query!(
        r#"DELETE FROM monitor_tags WHERE monitor_id = $1 AND tag_id = $2"#,
        monitor.0,
        tag.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_for_monitor(pool: &DbPool, monitor: MonitorId) -> DbResult<Vec<TagBrief>> {
    let rows = sqlx::query!(
        r#"
        SELECT t.id, t.name, t.color
        FROM tags t
        JOIN monitor_tags mt ON mt.tag_id = t.id
        WHERE mt.monitor_id = $1
        ORDER BY t.name
        "#,
        monitor.0,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| TagBrief {
            id: TagId::from_uuid(r.id),
            name: r.name,
            color: r.color,
        })
        .collect())
}

/// One round trip that returns tags for every channel in `ids`. Used by
/// the notification list endpoint so the UI can render routing tag chips
/// inline without per-row fetches.
pub async fn hydrate_for_channels(
    pool: &DbPool,
    ids: &[rampart_core::ids::NotificationId],
) -> DbResult<HashMap<rampart_core::ids::NotificationId, Vec<TagBrief>>> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let raw_ids: Vec<Uuid> = ids.iter().map(|n| n.0).collect();
    let rows = sqlx::query!(
        r#"
        SELECT nt.notification_id, t.id AS tag_id, t.name, t.color
        FROM notification_tags nt
        JOIN tags t ON t.id = nt.tag_id
        WHERE nt.notification_id = ANY($1)
        ORDER BY t.name
        "#,
        &raw_ids,
    )
    .fetch_all(pool)
    .await?;

    let mut by: HashMap<rampart_core::ids::NotificationId, Vec<TagBrief>> = HashMap::new();
    for r in rows {
        by.entry(rampart_core::ids::NotificationId::from_uuid(
            r.notification_id,
        ))
        .or_default()
        .push(TagBrief {
            id: TagId::from_uuid(r.tag_id),
            name: r.name,
            color: r.color,
        });
    }
    Ok(by)
}

/// One round trip that returns tags for every monitor in `ids`. Used
/// by the monitor list endpoint so we don't fan out N queries on a
/// large dashboard.
pub async fn hydrate_for_monitors(
    pool: &DbPool,
    ids: &[MonitorId],
) -> DbResult<HashMap<MonitorId, Vec<TagBrief>>> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let raw_ids: Vec<Uuid> = ids.iter().map(|m| m.0).collect();
    let rows = sqlx::query!(
        r#"
        SELECT mt.monitor_id, t.id AS tag_id, t.name, t.color
        FROM monitor_tags mt
        JOIN tags t ON t.id = mt.tag_id
        WHERE mt.monitor_id = ANY($1)
        ORDER BY t.name
        "#,
        &raw_ids,
    )
    .fetch_all(pool)
    .await?;

    let mut by: HashMap<MonitorId, Vec<TagBrief>> = HashMap::new();
    for r in rows {
        by.entry(MonitorId::from_uuid(r.monitor_id))
            .or_default()
            .push(TagBrief {
                id: TagId::from_uuid(r.tag_id),
                name: r.name,
                color: r.color,
            });
    }
    Ok(by)
}
