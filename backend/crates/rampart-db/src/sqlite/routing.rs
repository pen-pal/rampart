//! SQLite `routing` domain — the dispatch-path read `resolve_channels_for_monitor`:
//! the union of a monitor's explicitly-attached ∪ tag-matched ∪ folder-attached
//! channels, minus exclusions, active only. SQLite runs the same `WITH RECURSIVE`
//! folder-ancestor walk as PG, so the query ports near-verbatim; only the sqlx
//! macro type hints + the bool literal (`active = 1`) change. Reuses the
//! `sqlite::notifications` row decoder.

use super::notifications::{notification_from, COLS};
use crate::notifications::Notification;
use crate::DbResult;
use rampart_core::ids::MonitorId;
use sqlx::SqlitePool;

pub async fn resolve_channels_for_monitor(
    pool: &SqlitePool,
    monitor: MonitorId,
) -> DbResult<Vec<Notification>> {
    let m = monitor.0.to_string();
    let sql = format!(
        "WITH RECURSIVE mon AS (
            SELECT id, group_id FROM monitors WHERE id = ?
         ),
         ancestors AS (
            SELECT group_id AS gid FROM mon WHERE group_id IS NOT NULL
            UNION
            SELECT mg.parent_id AS gid
            FROM monitor_groups mg JOIN ancestors a ON mg.id = a.gid
            WHERE mg.parent_id IS NOT NULL
         ),
         eff_tags AS (
            SELECT tag_id FROM monitor_tags WHERE monitor_id = ?
            UNION
            SELECT gt.tag_id FROM group_tags gt WHERE gt.group_id IN (SELECT gid FROM ancestors)
         ),
         candidates AS (
            SELECT notification_id AS id FROM monitor_notifications WHERE monitor_id = ?
            UNION
            SELECT nt.notification_id AS id FROM notification_tags nt
            WHERE nt.tag_id IN (SELECT tag_id FROM eff_tags)
            UNION
            SELECT gn.notification_id AS id FROM group_notifications gn
            WHERE gn.group_id IN (SELECT gid FROM ancestors)
         )
         SELECT {COLS} FROM notifications
         WHERE active = 1
           AND id IN (SELECT id FROM candidates)
           AND id NOT IN (
               SELECT notification_id FROM monitor_notification_excludes WHERE monitor_id = ?
           )"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(&m)
        .bind(&m)
        .bind(&m)
        .bind(&m)
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(notification_from).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notifications::NewNotification;
    use crate::sqlite::{monitor_groups, notifications, silences};
    use uuid::Uuid;

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn dispatch_path_reads(pool: SqlitePool) {
        let org = super::super::oid(DEF);

        // Fresh DB: every dispatch read is a clean no-op, not a panic/error —
        // this is the boot case the notifier hits before any config exists.
        let mid = MonitorId::new();
        assert!(resolve_channels_for_monitor(&pool, mid)
            .await
            .unwrap()
            .is_empty());
        assert!(!silences::is_silenced(&pool, Some(mid.0)).await.unwrap());
        assert!(!silences::is_silenced(&pool, None).await.unwrap());
        assert!(!monitor_groups::any_parent_down(&pool, mid).await.unwrap());

        // Attach a channel to a real monitor → resolve returns it. Insert a
        // minimal monitors row directly (the FK target); routing only reads it.
        sqlx::query(
            "INSERT INTO monitors (id, name, kind, interval_seconds, timeout_seconds, org_id)
             VALUES (?, 'm', 'http', 60, 10, ?)",
        )
        .bind(mid.0.to_string())
        .bind(DEF)
        .execute(&pool)
        .await
        .unwrap();
        let ch = notifications::create(
            &pool,
            serde_json::from_value::<NewNotification>(serde_json::json!({
                "kind": "webhook", "name": "c", "config": {"url": "https://e.com/h"}
            }))
            .unwrap(),
            org,
        )
        .await
        .unwrap();
        notifications::attach(&pool, mid, ch.id).await.unwrap();
        let resolved = resolve_channels_for_monitor(&pool, mid).await.unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].id, ch.id);

        // An active global silence is observed.
        sqlx::query("INSERT INTO silences (id, reason, org_id) VALUES (?, 'x', ?)")
            .bind(Uuid::now_v7().to_string())
            .bind(DEF)
            .execute(&pool)
            .await
            .unwrap();
        assert!(silences::is_silenced(&pool, Some(mid.0)).await.unwrap());
    }
}
