//! Tag-based notification routing resolver.
//!
//! The notifier asks "which channels should fire for this monitor?" and
//! this module answers it live, per alert — no materialized routing rows.
//!
//!   effective_channels(monitor) =
//!       explicitly_attached(monitor)
//!     ∪ channels_sharing_a_tag(monitor.tags ∪ ancestor_folder_tags)
//!     ∪ ancestor_folder_attached_channels
//!     − explicitly_excluded(monitor)
//!
//! `ancestor_folder` is the monitor's direct folder plus every folder up
//! the parent chain to root — so a tag on a top-level "Production" folder
//! propagates down to monitors inside its nested "Databases" sub-folder.
//! Only `active` channels are returned. Exclusions always win.

use crate::notifications::Notification;
use crate::{DbPool, DbResult};
use rampart_core::ids::{MonitorGroupId, MonitorId, NotificationId, NotificationTemplateId, TagId};
use rampart_core::notification::ChannelKind;

/// Resolve the channels that should receive an alert for `monitor`.
/// Returns the same `Notification` shape as `notifications::for_monitor`
/// so the notifier can swap one for the other.
pub async fn resolve_channels_for_monitor(
    pool: &DbPool,
    monitor: MonitorId,
) -> DbResult<Vec<Notification>> {
    // One query, CTE-built. `candidates` is the union of the three
    // inclusion paths; we subtract the exclusion set and keep only active
    // channels, distinct.
    let rows = sqlx::query!(
        r#"
        WITH RECURSIVE mon AS (
            SELECT id, group_id FROM monitors WHERE id = $1
        ),
        -- The monitor's direct folder plus every folder up the parent chain.
        -- `UNION` (not UNION ALL) terminates on any data-level cycle, but the
        -- application-level cycle guard at write time should prevent those.
        ancestors AS (
            SELECT group_id AS gid FROM mon WHERE group_id IS NOT NULL
            UNION
            SELECT mg.parent_id AS gid
            FROM monitor_groups mg
            JOIN ancestors a ON mg.id = a.gid
            WHERE mg.parent_id IS NOT NULL
        ),
        -- Tags on the monitor + tags on every ancestor folder.
        eff_tags AS (
            SELECT tag_id FROM monitor_tags WHERE monitor_id = $1
            UNION
            SELECT gt.tag_id
            FROM group_tags gt
            WHERE gt.group_id IN (SELECT gid FROM ancestors)
        ),
        candidates AS (
            -- explicitly attached
            SELECT notification_id AS id FROM monitor_notifications WHERE monitor_id = $1
            UNION
            -- tag-matched: channel shares any effective tag
            SELECT nt.notification_id AS id
            FROM notification_tags nt
            WHERE nt.tag_id IN (SELECT tag_id FROM eff_tags)
            UNION
            -- attached to any ancestor folder
            SELECT gn.notification_id AS id
            FROM group_notifications gn
            WHERE gn.group_id IN (SELECT gid FROM ancestors)
        )
        SELECT n.id, n.kind AS "kind: ChannelKind", n.name, n.config, n.active,
               n.template_id, n.created_at, n.cooldown_seconds, n.last_fired_at
        FROM notifications n
        JOIN candidates c ON c.id = n.id
        WHERE n.active
          AND n.id NOT IN (
              SELECT notification_id FROM monitor_notification_excludes WHERE monitor_id = $1
          )
        "#,
        monitor.0,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| Notification {
            id: NotificationId::from_uuid(r.id),
            kind: r.kind,
            name: r.name,
            config: r.config,
            active: r.active,
            template_id: r.template_id.map(NotificationTemplateId::from_uuid),
            created_at: r.created_at,
            cooldown_seconds: r.cooldown_seconds,
            last_fired_at: r.last_fired_at,
            tags: Vec::new(),
        })
        .collect())
}

// ── read helpers (for the UI) ─────────────────────────────────────────────

pub async fn group_tag_ids(pool: &DbPool, group: MonitorGroupId) -> DbResult<Vec<TagId>> {
    let rows = sqlx::query!(
        r#"SELECT tag_id FROM group_tags WHERE group_id = $1"#,
        group.0
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| TagId::from_uuid(r.tag_id))
        .collect())
}

pub async fn channel_tag_ids(pool: &DbPool, notif: NotificationId) -> DbResult<Vec<TagId>> {
    let rows = sqlx::query!(
        r#"SELECT tag_id FROM notification_tags WHERE notification_id = $1"#,
        notif.0
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| TagId::from_uuid(r.tag_id))
        .collect())
}

pub async fn group_channel_ids(
    pool: &DbPool,
    group: MonitorGroupId,
) -> DbResult<Vec<NotificationId>> {
    let rows = sqlx::query!(
        r#"SELECT notification_id FROM group_notifications WHERE group_id = $1"#,
        group.0
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| NotificationId::from_uuid(r.notification_id))
        .collect())
}

pub async fn monitor_exclude_ids(
    pool: &DbPool,
    monitor: MonitorId,
) -> DbResult<Vec<NotificationId>> {
    let rows = sqlx::query!(
        r#"SELECT notification_id FROM monitor_notification_excludes WHERE monitor_id = $1"#,
        monitor.0
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| NotificationId::from_uuid(r.notification_id))
        .collect())
}

// ── junction helpers (used by API routes + tests) ─────────────────────────

pub async fn tag_group(pool: &DbPool, group: MonitorGroupId, tag: TagId) -> DbResult<()> {
    sqlx::query!(
        r#"INSERT INTO group_tags (group_id, tag_id) VALUES ($1, $2) ON CONFLICT DO NOTHING"#,
        group.0,
        tag.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn untag_group(pool: &DbPool, group: MonitorGroupId, tag: TagId) -> DbResult<()> {
    sqlx::query!(
        r#"DELETE FROM group_tags WHERE group_id = $1 AND tag_id = $2"#,
        group.0,
        tag.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn tag_channel(pool: &DbPool, notif: NotificationId, tag: TagId) -> DbResult<()> {
    sqlx::query!(
        r#"INSERT INTO notification_tags (notification_id, tag_id) VALUES ($1, $2) ON CONFLICT DO NOTHING"#,
        notif.0, tag.0,
    ).execute(pool).await?;
    Ok(())
}

pub async fn untag_channel(pool: &DbPool, notif: NotificationId, tag: TagId) -> DbResult<()> {
    sqlx::query!(
        r#"DELETE FROM notification_tags WHERE notification_id = $1 AND tag_id = $2"#,
        notif.0,
        tag.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn attach_group_channel(
    pool: &DbPool,
    group: MonitorGroupId,
    notif: NotificationId,
) -> DbResult<()> {
    sqlx::query!(
        r#"INSERT INTO group_notifications (group_id, notification_id) VALUES ($1, $2) ON CONFLICT DO NOTHING"#,
        group.0, notif.0,
    ).execute(pool).await?;
    Ok(())
}

pub async fn detach_group_channel(
    pool: &DbPool,
    group: MonitorGroupId,
    notif: NotificationId,
) -> DbResult<()> {
    sqlx::query!(
        r#"DELETE FROM group_notifications WHERE group_id = $1 AND notification_id = $2"#,
        group.0,
        notif.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Exclude a channel from a monitor — wins over any inclusion path.
pub async fn exclude_channel(
    pool: &DbPool,
    monitor: MonitorId,
    notif: NotificationId,
) -> DbResult<()> {
    sqlx::query!(
        r#"INSERT INTO monitor_notification_excludes (monitor_id, notification_id) VALUES ($1, $2) ON CONFLICT DO NOTHING"#,
        monitor.0, notif.0,
    ).execute(pool).await?;
    Ok(())
}

pub async fn unexclude_channel(
    pool: &DbPool,
    monitor: MonitorId,
    notif: NotificationId,
) -> DbResult<()> {
    sqlx::query!(
        r#"DELETE FROM monitor_notification_excludes WHERE monitor_id = $1 AND notification_id = $2"#,
        monitor.0, notif.0,
    ).execute(pool).await?;
    Ok(())
}
