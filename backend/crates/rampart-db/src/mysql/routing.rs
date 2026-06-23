//! MySQL `routing` domain — the dispatch-path read `resolve_channels_for_monitor`:
//! the union of a monitor's explicitly-attached ∪ tag-matched ∪ folder-attached
//! channels, minus exclusions, active only. MariaDB 10.2+/MySQL 8 run the same
//! `WITH RECURSIVE` folder-ancestor walk as PG/SQLite, so the query ports
//! near-verbatim (`active = 1`). Reuses the `mysql::notifications` row decoder.

use super::notifications::{notification_from, COLS};
use crate::notifications::Notification;
use crate::DbResult;
use rampart_core::ids::{MonitorGroupId, MonitorId, NotificationId, TagId};
use sqlx::{MySqlPool, Row};
use uuid::Uuid;

pub async fn resolve_channels_for_monitor(
    pool: &MySqlPool,
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

// ── read helpers (for the UI) ─────────────────────────────────────────────

async fn id_col(pool: &MySqlPool, sql: &'static str, bind: &str) -> DbResult<Vec<Uuid>> {
    let rows = sqlx::query(sql).bind(bind).fetch_all(pool).await?;
    Ok(rows
        .iter()
        .map(|r| super::raw_uuid(&r.get::<String, _>(0)))
        .collect())
}

pub async fn group_tag_ids(pool: &MySqlPool, group: MonitorGroupId) -> DbResult<Vec<TagId>> {
    let ids = id_col(
        pool,
        "SELECT tag_id FROM group_tags WHERE group_id = ?",
        &group.0.to_string(),
    )
    .await?;
    Ok(ids.into_iter().map(TagId::from_uuid).collect())
}

pub async fn channel_tag_ids(pool: &MySqlPool, notif: NotificationId) -> DbResult<Vec<TagId>> {
    let ids = id_col(
        pool,
        "SELECT tag_id FROM notification_tags WHERE notification_id = ?",
        &notif.0.to_string(),
    )
    .await?;
    Ok(ids.into_iter().map(TagId::from_uuid).collect())
}

pub async fn group_channel_ids(
    pool: &MySqlPool,
    group: MonitorGroupId,
) -> DbResult<Vec<NotificationId>> {
    let ids = id_col(
        pool,
        "SELECT notification_id FROM group_notifications WHERE group_id = ?",
        &group.0.to_string(),
    )
    .await?;
    Ok(ids.into_iter().map(NotificationId::from_uuid).collect())
}

pub async fn monitor_exclude_ids(
    pool: &MySqlPool,
    monitor: MonitorId,
) -> DbResult<Vec<NotificationId>> {
    let ids = id_col(
        pool,
        "SELECT notification_id FROM monitor_notification_excludes WHERE monitor_id = ?",
        &monitor.0.to_string(),
    )
    .await?;
    Ok(ids.into_iter().map(NotificationId::from_uuid).collect())
}

// ── junction helpers ──────────────────────────────────────────────────────

async fn link(pool: &MySqlPool, sql: &'static str, a: &str, b: &str) -> DbResult<()> {
    sqlx::query(sql).bind(a).bind(b).execute(pool).await?;
    Ok(())
}

pub async fn tag_group(pool: &MySqlPool, group: MonitorGroupId, tag: TagId) -> DbResult<()> {
    link(
        pool,
        "INSERT IGNORE INTO group_tags (group_id, tag_id) VALUES (?, ?)",
        &group.0.to_string(),
        &tag.0.to_string(),
    )
    .await
}

pub async fn untag_group(pool: &MySqlPool, group: MonitorGroupId, tag: TagId) -> DbResult<()> {
    link(
        pool,
        "DELETE FROM group_tags WHERE group_id = ? AND tag_id = ?",
        &group.0.to_string(),
        &tag.0.to_string(),
    )
    .await
}

pub async fn tag_channel(pool: &MySqlPool, notif: NotificationId, tag: TagId) -> DbResult<()> {
    link(
        pool,
        "INSERT IGNORE INTO notification_tags (notification_id, tag_id) VALUES (?, ?)",
        &notif.0.to_string(),
        &tag.0.to_string(),
    )
    .await
}

pub async fn untag_channel(pool: &MySqlPool, notif: NotificationId, tag: TagId) -> DbResult<()> {
    link(
        pool,
        "DELETE FROM notification_tags WHERE notification_id = ? AND tag_id = ?",
        &notif.0.to_string(),
        &tag.0.to_string(),
    )
    .await
}

pub async fn attach_group_channel(
    pool: &MySqlPool,
    group: MonitorGroupId,
    notif: NotificationId,
) -> DbResult<()> {
    link(
        pool,
        "INSERT IGNORE INTO group_notifications (group_id, notification_id) VALUES (?, ?)",
        &group.0.to_string(),
        &notif.0.to_string(),
    )
    .await
}

pub async fn detach_group_channel(
    pool: &MySqlPool,
    group: MonitorGroupId,
    notif: NotificationId,
) -> DbResult<()> {
    link(
        pool,
        "DELETE FROM group_notifications WHERE group_id = ? AND notification_id = ?",
        &group.0.to_string(),
        &notif.0.to_string(),
    )
    .await
}

/// Exclude a channel from a monitor — wins over any inclusion path.
pub async fn exclude_channel(
    pool: &MySqlPool,
    monitor: MonitorId,
    notif: NotificationId,
) -> DbResult<()> {
    link(
        pool,
        "INSERT IGNORE INTO monitor_notification_excludes (monitor_id, notification_id) VALUES (?, ?)",
        &monitor.0.to_string(),
        &notif.0.to_string(),
    )
    .await
}

pub async fn unexclude_channel(
    pool: &MySqlPool,
    monitor: MonitorId,
    notif: NotificationId,
) -> DbResult<()> {
    link(
        pool,
        "DELETE FROM monitor_notification_excludes WHERE monitor_id = ? AND notification_id = ?",
        &monitor.0.to_string(),
        &notif.0.to_string(),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mysql::{monitor_groups, monitors, notifications, silences};
    use crate::notifications::NewNotification;
    use rampart_core::monitor::{MonitorKind, NewMonitor};
    use uuid::Uuid;

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    fn new_http() -> NewMonitor {
        NewMonitor {
            name: "m".into(),
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
        }
    }

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn dispatch_path_reads(pool: MySqlPool) {
        let org = super::super::oid(DEF);

        // Fresh-ish DB: every dispatch read is a clean no-op, not a panic — the
        // boot case the notifier hits before any config exists.
        let absent = MonitorId::new();
        assert!(resolve_channels_for_monitor(&pool, absent)
            .await
            .unwrap()
            .is_empty());
        assert!(!silences::is_silenced(&pool, Some(absent.0)).await.unwrap());
        assert!(!silences::is_silenced(&pool, None).await.unwrap());
        assert!(!monitor_groups::any_parent_down(&pool, absent)
            .await
            .unwrap());

        // Attach a channel to a real monitor → resolve returns it.
        let mon = monitors::create(&pool, new_http(), org).await.unwrap();
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
        notifications::attach(&pool, mon.id, ch.id).await.unwrap();
        let resolved = resolve_channels_for_monitor(&pool, mon.id).await.unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].id, ch.id);

        // An active global silence is observed.
        sqlx::query("INSERT INTO silences (id, reason, org_id) VALUES (?, 'x', ?)")
            .bind(Uuid::now_v7().to_string())
            .bind(DEF)
            .execute(&pool)
            .await
            .unwrap();
        assert!(silences::is_silenced(&pool, Some(mon.id.0)).await.unwrap());
    }

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn junction_round_trips(pool: MySqlPool) {
        use crate::mysql::tags;
        let org = super::super::oid(DEF);
        let grp = monitor_groups::create(
            &pool,
            rampart_core::monitor_group::NewMonitorGroup {
                name: "G".into(),
                sort_order: 0,
                parent_id: None,
            },
            org,
        )
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
        let tag = tags::create(
            &pool,
            serde_json::from_value(serde_json::json!({ "name": "env", "color": "#ffffff" }))
                .unwrap(),
            org,
        )
        .await
        .unwrap();
        let mon = monitors::create(&pool, new_http(), org).await.unwrap();

        // group ↔ tag (idempotent insert + read + delete).
        tag_group(&pool, grp.id, tag.id).await.unwrap();
        tag_group(&pool, grp.id, tag.id).await.unwrap();
        assert_eq!(group_tag_ids(&pool, grp.id).await.unwrap(), vec![tag.id]);
        untag_group(&pool, grp.id, tag.id).await.unwrap();
        assert!(group_tag_ids(&pool, grp.id).await.unwrap().is_empty());

        // channel ↔ tag.
        tag_channel(&pool, ch.id, tag.id).await.unwrap();
        assert_eq!(channel_tag_ids(&pool, ch.id).await.unwrap(), vec![tag.id]);
        untag_channel(&pool, ch.id, tag.id).await.unwrap();
        assert!(channel_tag_ids(&pool, ch.id).await.unwrap().is_empty());

        // group ↔ channel.
        attach_group_channel(&pool, grp.id, ch.id).await.unwrap();
        assert_eq!(group_channel_ids(&pool, grp.id).await.unwrap(), vec![ch.id]);
        detach_group_channel(&pool, grp.id, ch.id).await.unwrap();
        assert!(group_channel_ids(&pool, grp.id).await.unwrap().is_empty());

        // monitor channel exclusion.
        exclude_channel(&pool, mon.id, ch.id).await.unwrap();
        assert_eq!(
            monitor_exclude_ids(&pool, mon.id).await.unwrap(),
            vec![ch.id]
        );
        unexclude_channel(&pool, mon.id, ch.id).await.unwrap();
        assert!(monitor_exclude_ids(&pool, mon.id).await.unwrap().is_empty());
    }
}
