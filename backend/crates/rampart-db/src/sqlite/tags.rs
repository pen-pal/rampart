//! SQLite `tags` domain — colored labels attached to monitors (and, via the
//! routing join tables, to channels/groups). Mirrors the Postgres
//! `crate::tags` free-fn surface: list / get / create / update / usage /
//! delete / attach / detach / list_for_monitor / hydrate_for_{channels,
//! monitors}.
//!
//! Dialect notes: uuids are TEXT; `created_at` is INTEGER unix-seconds;
//! per-org name uniqueness (PG's `tags_org_name_uidx`) maps a violation to
//! [`DbError::Conflict`]. No `= ANY($1)` on SQLite, so the batch hydrators
//! build a `IN (?,?,…)` list with bound params (still injection-safe — the
//! placeholder count is derived from the slice, never interpolated values).

use super::{mid, raw_uuid, ts};
use crate::{DbError, DbResult};
use rampart_core::ids::{NotificationId, OrgId};
use rampart_core::tag::{NewTag, Tag, TagBrief, UpdateTag};
use rampart_core::{MonitorId, TagId};
use sqlx::{Row, SqlitePool};
use std::collections::HashMap;

/// Parse a TEXT uuid column into a `TagId`.
fn tid(s: &str) -> TagId {
    TagId::from_uuid(raw_uuid(s))
}

/// A unique-constraint hit on `(org_id, name)` → a friendly Conflict.
fn name_conflict(e: sqlx::Error) -> DbError {
    match e {
        sqlx::Error::Database(d) if d.is_unique_violation() => {
            DbError::Conflict("tag name is already in use".into())
        }
        other => DbError::from(other),
    }
}

fn tag_from(r: &sqlx::sqlite::SqliteRow) -> Tag {
    Tag {
        id: tid(&r.get::<String, _>("id")),
        name: r.get("name"),
        color: r.get("color"),
        created_at: ts(r.get::<i64, _>("created_at")),
    }
}

fn brief_from(r: &sqlx::sqlite::SqliteRow) -> TagBrief {
    TagBrief {
        id: tid(&r.get::<String, _>("id")),
        name: r.get("name"),
        color: r.get("color"),
    }
}

pub async fn list(pool: &SqlitePool, org_id: OrgId) -> DbResult<Vec<Tag>> {
    let rows =
        sqlx::query("SELECT id, name, color, created_at FROM tags WHERE org_id = ? ORDER BY name")
            .bind(org_id.0.to_string())
            .fetch_all(pool)
            .await?;
    Ok(rows.iter().map(tag_from).collect())
}

pub async fn get(pool: &SqlitePool, id: TagId, org_id: OrgId) -> DbResult<Tag> {
    let row =
        sqlx::query("SELECT id, name, color, created_at FROM tags WHERE id = ? AND org_id = ?")
            .bind(id.0.to_string())
            .bind(org_id.0.to_string())
            .fetch_optional(pool)
            .await?
            .ok_or(DbError::NotFound)?;
    Ok(tag_from(&row))
}

pub async fn create(pool: &SqlitePool, input: NewTag, org_id: OrgId) -> DbResult<Tag> {
    let id = TagId::new();
    let row = sqlx::query(
        "INSERT INTO tags (id, name, color, org_id)
         VALUES (?, ?, ?, ?)
         RETURNING id, name, color, created_at",
    )
    .bind(id.0.to_string())
    .bind(input.name)
    .bind(input.color)
    .bind(org_id.0.to_string())
    .fetch_one(pool)
    .await
    .map_err(name_conflict)?;
    Ok(tag_from(&row))
}

pub async fn update(
    pool: &SqlitePool,
    id: TagId,
    patch: UpdateTag,
    org_id: OrgId,
) -> DbResult<Tag> {
    let res = sqlx::query(
        "UPDATE tags
            SET name  = COALESCE(?, name),
                color = COALESCE(?, color)
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
        return Err(DbError::NotFound);
    }
    get(pool, id, org_id).await
}

/// Per-tag usage counts (monitors / channels / groups). Mirrors PG `usage`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TagUsage {
    pub tag_id: TagId,
    pub monitors: i64,
    pub channels: i64,
    pub groups: i64,
}

pub async fn usage(pool: &SqlitePool, org_id: OrgId) -> DbResult<Vec<TagUsage>> {
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

pub async fn delete(pool: &SqlitePool, id: TagId, org_id: OrgId) -> DbResult<()> {
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

pub async fn attach(pool: &SqlitePool, monitor: MonitorId, tag: TagId) -> DbResult<()> {
    sqlx::query(
        "INSERT INTO monitor_tags (monitor_id, tag_id) VALUES (?, ?) ON CONFLICT DO NOTHING",
    )
    .bind(monitor.0.to_string())
    .bind(tag.0.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn detach(pool: &SqlitePool, monitor: MonitorId, tag: TagId) -> DbResult<()> {
    sqlx::query("DELETE FROM monitor_tags WHERE monitor_id = ? AND tag_id = ?")
        .bind(monitor.0.to_string())
        .bind(tag.0.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_for_monitor(pool: &SqlitePool, monitor: MonitorId) -> DbResult<Vec<TagBrief>> {
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

/// `IN (?,?,…)` placeholder list for a slice of length `n` (n >= 1). The count
/// comes from the slice, the values are bound — no value is ever interpolated,
/// so this is injection-safe despite building the SQL string.
fn in_placeholders(n: usize) -> String {
    let mut s = String::with_capacity(n * 2);
    for i in 0..n {
        if i > 0 {
            s.push(',');
        }
        s.push('?');
    }
    s
}

pub async fn hydrate_for_channels(
    pool: &SqlitePool,
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
    pool: &SqlitePool,
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
    use crate::sqlite::monitors;
    use rampart_core::monitor::{MonitorKind, NewMonitor};

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    fn new_tag(name: &str) -> NewTag {
        NewTag {
            name: name.into(),
            color: "#ff0000".into(),
        }
    }

    async fn a_monitor(pool: &SqlitePool, name: &str) -> MonitorId {
        let org = super::super::oid(DEF);
        monitors::create(
            pool,
            NewMonitor {
                name: name.into(),
                kind: MonitorKind::Http,
                url: Some("https://x".into()),
                hostname: None,
                port: None,
                config: serde_json::json!({}),
                interval_seconds: 60,
                timeout_seconds: 10,
                max_retries: 0,
                retry_interval_sec: 60,
                resend_interval_sec: 0,
                upside_down: false,
                http_method: "GET".into(),
                http_body: None,
                http_headers: None,
                accepted_statuses: vec![200],
                follow_redirect: true,
                ignore_tls: false,
                proxy_id: None,
                group_id: None,
                check_cert: false,
                cert_expiry_days: 14,
                slo_target_pct: None,
                slo_window_days: None,
                agent_id: None,
                escalation_policy_id: None,
            },
            org,
        )
        .await
        .unwrap()
        .id
    }

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn crud_and_conflict(pool: SqlitePool) {
        let org = super::super::oid(DEF);
        let t = create(&pool, new_tag("prod"), org).await.unwrap();
        assert_eq!(t.name, "prod");
        assert_eq!(list(&pool, org).await.unwrap().len(), 1);
        assert_eq!(get(&pool, t.id, org).await.unwrap().color, "#ff0000");

        // Per-org name uniqueness → Conflict.
        assert!(matches!(
            create(&pool, new_tag("prod"), org).await,
            Err(DbError::Conflict(_))
        ));

        // Update (rename) then delete.
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
        assert_eq!(u.color, "#ff0000"); // untouched
        delete(&pool, t.id, org).await.unwrap();
        assert!(matches!(
            get(&pool, t.id, org).await,
            Err(DbError::NotFound)
        ));
    }

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn attach_hydrate_usage_and_monitor_flip(pool: SqlitePool) {
        let org = super::super::oid(DEF);
        let m1 = a_monitor(&pool, "m1").await;
        let m2 = a_monitor(&pool, "m2").await;
        let prod = create(&pool, new_tag("prod"), org).await.unwrap();
        let web = create(&pool, new_tag("web"), org).await.unwrap();

        attach(&pool, m1, prod.id).await.unwrap();
        attach(&pool, m1, web.id).await.unwrap();
        attach(&pool, m2, prod.id).await.unwrap();
        // attach is idempotent.
        attach(&pool, m1, prod.id).await.unwrap();

        // list_for_monitor (sorted by name).
        let m1_tags = list_for_monitor(&pool, m1).await.unwrap();
        assert_eq!(m1_tags.len(), 2);
        assert_eq!(m1_tags[0].name, "prod");
        assert_eq!(m1_tags[1].name, "web");

        // batch hydrate over both monitors.
        let map = hydrate_for_monitors(&pool, &[m1, m2]).await.unwrap();
        assert_eq!(map.get(&m1).unwrap().len(), 2);
        assert_eq!(map.get(&m2).unwrap().len(), 1);

        // monitor read carries hydrated tags now (deferred-fn closure).
        assert_eq!(monitors::get(&pool, m1, org).await.unwrap().tags.len(), 2);

        // usage counts.
        let u = usage(&pool, org).await.unwrap();
        let prod_use = u.iter().find(|x| x.tag_id == prod.id).unwrap();
        assert_eq!(prod_use.monitors, 2);
        assert_eq!(prod_use.channels, 0);

        // set_active_by_tag flips only the tagged monitors; returns the count.
        let flipped = monitors::set_active_by_tag(&pool, prod.id, false, org)
            .await
            .unwrap();
        assert_eq!(flipped, 2);
        assert!(!monitors::get(&pool, m1, org).await.unwrap().active);
        // idempotent: second flip touches nothing (active already matches).
        assert_eq!(
            monitors::set_active_by_tag(&pool, prod.id, false, org)
                .await
                .unwrap(),
            0
        );

        // detach drops the link.
        detach(&pool, m1, web.id).await.unwrap();
        assert_eq!(list_for_monitor(&pool, m1).await.unwrap().len(), 1);
    }
}
