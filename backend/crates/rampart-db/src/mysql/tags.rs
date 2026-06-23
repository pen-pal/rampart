//! MySQL `tags` domain — colored labels on monitors (and channels/groups via
//! the routing join tables). Mirrors the PG/SQLite surface. MySQL deltas: no
//! `RETURNING` (insert-then-get on create); `IN (?,…)` batch hydrators (no array
//! binds); per-org name uniqueness → `Conflict`.

use super::{in_placeholders, mid, raw_uuid, ts};
use crate::tags::TagUsage;
use crate::{DbError, DbResult};
use rampart_core::ids::{NotificationId, OrgId};
use rampart_core::tag::{NewTag, Tag, TagBrief, UpdateTag};
use rampart_core::{MonitorId, TagId};
use sqlx::{MySqlPool, Row};
use std::collections::HashMap;

fn tid(s: &str) -> TagId {
    TagId::from_uuid(raw_uuid(s))
}

fn name_conflict(e: sqlx::Error) -> DbError {
    match e {
        sqlx::Error::Database(d) if d.is_unique_violation() => {
            DbError::Conflict("tag name is already in use".into())
        }
        other => DbError::from(other),
    }
}

fn tag_from(r: &sqlx::mysql::MySqlRow) -> Tag {
    Tag {
        id: tid(&r.get::<String, _>("id")),
        name: r.get("name"),
        color: r.get("color"),
        created_at: ts(r.get::<i64, _>("created_at")),
    }
}

fn brief_from(r: &sqlx::mysql::MySqlRow) -> TagBrief {
    TagBrief {
        id: tid(&r.get::<String, _>("id")),
        name: r.get("name"),
        color: r.get("color"),
    }
}

pub async fn list(pool: &MySqlPool, org_id: OrgId) -> DbResult<Vec<Tag>> {
    let rows =
        sqlx::query("SELECT id, name, color, created_at FROM tags WHERE org_id = ? ORDER BY name")
            .bind(org_id.0.to_string())
            .fetch_all(pool)
            .await?;
    Ok(rows.iter().map(tag_from).collect())
}

pub async fn get(pool: &MySqlPool, id: TagId, org_id: OrgId) -> DbResult<Tag> {
    let row =
        sqlx::query("SELECT id, name, color, created_at FROM tags WHERE id = ? AND org_id = ?")
            .bind(id.0.to_string())
            .bind(org_id.0.to_string())
            .fetch_optional(pool)
            .await?
            .ok_or(DbError::NotFound)?;
    Ok(tag_from(&row))
}

pub async fn create(pool: &MySqlPool, input: NewTag, org_id: OrgId) -> DbResult<Tag> {
    let id = TagId::new();
    sqlx::query("INSERT INTO tags (id, name, color, org_id) VALUES (?, ?, ?, ?)")
        .bind(id.0.to_string())
        .bind(input.name)
        .bind(input.color)
        .bind(org_id.0.to_string())
        .execute(pool)
        .await
        .map_err(name_conflict)?;
    get(pool, id, org_id).await
}

pub async fn update(pool: &MySqlPool, id: TagId, patch: UpdateTag, org_id: OrgId) -> DbResult<Tag> {
    let res = sqlx::query(
        "UPDATE tags SET name = COALESCE(?, name), color = COALESCE(?, color)
          WHERE id = ? AND org_id = ?",
    )
    .bind(patch.name)
    .bind(patch.color)
    .bind(id.0.to_string())
    .bind(org_id.0.to_string())
    .execute(pool)
    .await
    .map_err(name_conflict)?;
    if res.rows_affected() == 0 {
        // Could be NotFound or a no-op rename; disambiguate with a read.
        return get(pool, id, org_id).await;
    }
    get(pool, id, org_id).await
}

pub async fn usage(pool: &MySqlPool, org_id: OrgId) -> DbResult<Vec<TagUsage>> {
    let rows = sqlx::query(
        "SELECT t.id AS tag_id,
                (SELECT COUNT(*) FROM monitor_tags      m WHERE m.tag_id = t.id) AS monitors,
                (SELECT COUNT(*) FROM notification_tags n WHERE n.tag_id = t.id) AS channels,
                (SELECT COUNT(*) FROM group_tags        g WHERE g.tag_id = t.id) AS groups_
         FROM tags t
         WHERE t.org_id = ?",
    )
    .bind(org_id.0.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| TagUsage {
            tag_id: tid(&r.get::<String, _>("tag_id")),
            monitors: r.get("monitors"),
            channels: r.get("channels"),
            groups: r.get("groups_"),
        })
        .collect())
}

pub async fn delete(pool: &MySqlPool, id: TagId, org_id: OrgId) -> DbResult<()> {
    let res = sqlx::query("DELETE FROM tags WHERE id = ? AND org_id = ?")
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

pub async fn attach(pool: &MySqlPool, monitor: MonitorId, tag: TagId) -> DbResult<()> {
    sqlx::query(
        "INSERT INTO monitor_tags (monitor_id, tag_id) VALUES (?, ?)
         ON DUPLICATE KEY UPDATE monitor_id = monitor_id",
    )
    .bind(monitor.0.to_string())
    .bind(tag.0.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn detach(pool: &MySqlPool, monitor: MonitorId, tag: TagId) -> DbResult<()> {
    sqlx::query("DELETE FROM monitor_tags WHERE monitor_id = ? AND tag_id = ?")
        .bind(monitor.0.to_string())
        .bind(tag.0.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_for_monitor(pool: &MySqlPool, monitor: MonitorId) -> DbResult<Vec<TagBrief>> {
    let rows = sqlx::query(
        "SELECT t.id, t.name, t.color
         FROM tags t JOIN monitor_tags mt ON mt.tag_id = t.id
         WHERE mt.monitor_id = ? ORDER BY t.name",
    )
    .bind(monitor.0.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(brief_from).collect())
}

pub async fn hydrate_for_channels(
    pool: &MySqlPool,
    ids: &[NotificationId],
) -> DbResult<HashMap<NotificationId, Vec<TagBrief>>> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let sql = format!(
        "SELECT nt.notification_id, t.id, t.name, t.color
         FROM notification_tags nt JOIN tags t ON t.id = nt.tag_id
         WHERE nt.notification_id IN ({}) ORDER BY t.name",
        in_placeholders(ids.len())
    );
    let mut q = sqlx::query(sqlx::AssertSqlSafe(sql));
    for id in ids {
        q = q.bind(id.0.to_string());
    }
    let rows = q.fetch_all(pool).await?;
    let mut by: HashMap<NotificationId, Vec<TagBrief>> = HashMap::new();
    for r in &rows {
        by.entry(NotificationId::from_uuid(raw_uuid(
            &r.get::<String, _>("notification_id"),
        )))
        .or_default()
        .push(brief_from(r));
    }
    Ok(by)
}

pub async fn hydrate_for_monitors(
    pool: &MySqlPool,
    ids: &[MonitorId],
) -> DbResult<HashMap<MonitorId, Vec<TagBrief>>> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let sql = format!(
        "SELECT mt.monitor_id, t.id, t.name, t.color
         FROM monitor_tags mt JOIN tags t ON t.id = mt.tag_id
         WHERE mt.monitor_id IN ({}) ORDER BY t.name",
        in_placeholders(ids.len())
    );
    let mut q = sqlx::query(sqlx::AssertSqlSafe(sql));
    for id in ids {
        q = q.bind(id.0.to_string());
    }
    let rows = q.fetch_all(pool).await?;
    let mut by: HashMap<MonitorId, Vec<TagBrief>> = HashMap::new();
    for r in &rows {
        by.entry(mid(&r.get::<String, _>("monitor_id")))
            .or_default()
            .push(brief_from(r));
    }
    Ok(by)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    fn new_tag(name: &str) -> NewTag {
        NewTag {
            name: name.into(),
            color: "#ff0000".into(),
        }
    }

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn crud_and_conflict(pool: MySqlPool) {
        let org = super::super::oid(DEF);
        let t = create(&pool, new_tag("prod"), org).await.unwrap();
        assert_eq!(t.name, "prod");
        assert_eq!(list(&pool, org).await.unwrap().len(), 1);
        assert_eq!(get(&pool, t.id, org).await.unwrap().color, "#ff0000");

        assert!(matches!(
            create(&pool, new_tag("prod"), org).await,
            Err(DbError::Conflict(_))
        ));

        let u = update(
            &pool,
            t.id,
            UpdateTag {
                name: Some("staging".into()),
                color: None,
            },
            org,
        )
        .await
        .unwrap();
        assert_eq!(u.name, "staging");
        assert_eq!(u.color, "#ff0000");
        delete(&pool, t.id, org).await.unwrap();
        assert!(matches!(
            get(&pool, t.id, org).await,
            Err(DbError::NotFound)
        ));
    }
}
