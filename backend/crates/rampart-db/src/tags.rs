//! Tag repository.

use crate::{DbError, DbPool, DbResult};
use rampart_core::tag::{NewTag, Tag, TagBrief};
use rampart_core::{MonitorId, TagId};
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

pub async fn list(pool: &DbPool) -> DbResult<Vec<Tag>> {
    let rows = sqlx::query_as!(
        TagRow,
        r#"SELECT id, name, color, created_at FROM tags ORDER BY name"#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn get(pool: &DbPool, id: TagId) -> DbResult<Tag> {
    let row = sqlx::query_as!(
        TagRow,
        r#"SELECT id, name, color, created_at FROM tags WHERE id = $1"#,
        id.0,
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

pub async fn delete(pool: &DbPool, id: TagId) -> DbResult<()> {
    let result = sqlx::query!("DELETE FROM tags WHERE id = $1", id.0)
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
