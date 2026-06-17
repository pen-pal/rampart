//! Notification channel queries.

use crate::{DbError, DbPool, DbResult};
use rampart_core::ids::{MonitorId, NotificationId, NotificationTemplateId, OrgId};
use rampart_core::tag::TagBrief;
use rampart_core::ChannelKind;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct Notification {
    pub id: NotificationId,
    pub kind: ChannelKind,
    pub name: String,
    pub config: serde_json::Value,
    pub active: bool,
    pub template_id: Option<NotificationTemplateId>,
    pub created_at: OffsetDateTime,
    #[serde(default)]
    pub cooldown_seconds: i32,
    /// Coalescing window in seconds. 0 = send each event immediately
    /// (legacy). > 0 = buffer events for this channel in the notifier and
    /// flush one combined message every `digest_window_secs`. Clamped to
    /// 0..=3600 on write.
    #[serde(default)]
    pub digest_window_secs: i32,
    /// Quiet-hours window in UTC, half-open `[start, end)`. Both must be
    /// `Some` for the check to apply; a non-Test send whose current UTC
    /// hour falls inside the window is dropped. Wrap-around (start > end)
    /// is supported. Clamped to 0..=23 on write.
    #[serde(default)]
    pub quiet_hours_start: Option<i16>,
    #[serde(default)]
    pub quiet_hours_end: Option<i16>,
    /// Max non-Test sends per rolling hour. 0 = unlimited (legacy).
    /// Enforced in-memory by the notifier. Clamped to >= 0 on write.
    #[serde(default)]
    pub rate_limit_per_hour: i32,
    #[serde(default)]
    pub last_fired_at: Option<OffsetDateTime>,
    /// Routing tags attached to this channel. Hydrated by the DB layer on
    /// list/get reads so the UI can render them inline without fanning out
    /// N follow-up requests. Empty by default for paths that don't need it
    /// (e.g. the routing resolver — the notifier itself doesn't display
    /// tags, only uses them for matching).
    #[serde(default)]
    pub tags: Vec<TagBrief>,
}

#[derive(Debug, Deserialize)]
pub struct NewNotification {
    pub kind: ChannelKind,
    pub name: String,
    pub config: serde_json::Value,
    #[serde(default = "default_enabled")]
    pub active: bool,
    #[serde(default)]
    pub template_id: Option<NotificationTemplateId>,
    /// 0 = no cooldown (legacy). Suppresses sends within N seconds of
    /// the channel's last successful fire.
    #[serde(default)]
    pub cooldown_seconds: i32,
    /// 0 = immediate (legacy). > 0 = coalesce events into one combined
    /// message every N seconds. Clamped to 0..=3600 on insert.
    #[serde(default)]
    pub digest_window_secs: i32,
    /// Quiet-hours window in UTC `[start, end)`. Both must be set for the
    /// check to apply. Clamped to 0..=23 on insert.
    #[serde(default)]
    pub quiet_hours_start: Option<i16>,
    #[serde(default)]
    pub quiet_hours_end: Option<i16>,
    /// 0 = unlimited (legacy). > 0 = max non-Test sends per rolling hour.
    #[serde(default)]
    pub rate_limit_per_hour: i32,
}
fn default_enabled() -> bool {
    true
}

/// Clamp a digest window to the supported 0..=3600 range. Mirrors the
/// DB CHECK constraint so a bad value is corrected rather than rejected.
fn clamp_digest_window(v: i32) -> i32 {
    v.clamp(0, 3600)
}

/// Clamp a quiet-hours hour into the 0..=23 range. `None` stays `None`.
/// Mirrors the DB CHECK so a bad value is corrected rather than rejected.
fn clamp_hour(v: Option<i16>) -> Option<i16> {
    v.map(|h| h.clamp(0, 23))
}

/// Clamp a rate limit to be non-negative. Mirrors the DB CHECK.
fn clamp_rate(v: i32) -> i32 {
    v.max(0)
}

#[derive(Debug, Deserialize)]
pub struct UpdateNotification {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub config: Option<serde_json::Value>,
    #[serde(default)]
    pub active: Option<bool>,
    /// Outer Option = "field present in payload". Inner Option = "explicit null
    /// → clear the assignment". Set None to leave unchanged.
    ///
    /// `#[serde(default, deserialize_with = "double_option")]` is the
    /// idiom to distinguish absent (None) from explicit `null` (Some(None)).
    #[serde(default, deserialize_with = "double_option")]
    pub template_id: Option<Option<NotificationTemplateId>>,
    #[serde(default)]
    pub cooldown_seconds: Option<i32>,
    #[serde(default)]
    pub digest_window_secs: Option<i32>,
    /// Outer Option = present in payload; inner Option = explicit null
    /// (clear the quiet-hours bound). Absent leaves the column unchanged.
    #[serde(default, deserialize_with = "double_option")]
    pub quiet_hours_start: Option<Option<i16>>,
    #[serde(default, deserialize_with = "double_option")]
    pub quiet_hours_end: Option<Option<i16>>,
    #[serde(default)]
    pub rate_limit_per_hour: Option<i32>,
}

/// Wraps the deserialized value in an outer `Some` so the caller can
/// distinguish "field absent" (`#[serde(default)]` gives `None`) from
/// "field present, value is null" (gives `Some(None)`).
fn double_option<'de, T, D>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    T: serde::Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    serde::Deserialize::deserialize(d).map(Some)
}

/// Notification channels owned by one org — the management list (org-scoped).
pub async fn list(pool: &DbPool, org_id: OrgId) -> DbResult<Vec<Notification>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, kind AS "kind: ChannelKind", name, config, active,
               template_id, created_at, cooldown_seconds, digest_window_secs,
               quiet_hours_start, quiet_hours_end, rate_limit_per_hour, last_fired_at
        FROM notifications
        WHERE org_id = $1
        ORDER BY created_at DESC
        "#,
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    let mut channels: Vec<Notification> = rows
        .into_iter()
        .map(|r| Notification {
            id: NotificationId::from_uuid(r.id),
            kind: r.kind,
            name: r.name,
            config: crate::secrets::open(r.config),
            active: r.active,
            template_id: r.template_id.map(NotificationTemplateId::from_uuid),
            created_at: r.created_at,
            cooldown_seconds: r.cooldown_seconds,
            digest_window_secs: r.digest_window_secs,
            quiet_hours_start: r.quiet_hours_start,
            quiet_hours_end: r.quiet_hours_end,
            rate_limit_per_hour: r.rate_limit_per_hour,
            last_fired_at: r.last_fired_at,
            tags: Vec::new(),
        })
        .collect();
    let ids: Vec<NotificationId> = channels.iter().map(|c| c.id).collect();
    let mut tag_map = crate::tags::hydrate_for_channels(pool, &ids).await?;
    for c in channels.iter_mut() {
        if let Some(t) = tag_map.remove(&c.id) {
            c.tags = t;
        }
    }
    Ok(channels)
}

/// Every channel across ALL orgs — for system callers (seed/import). Not
/// org-scoped; never call on a request path (use [`list`]).
pub async fn list_all(pool: &DbPool) -> DbResult<Vec<Notification>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, kind AS "kind: ChannelKind", name, config, active,
               template_id, created_at, cooldown_seconds, digest_window_secs,
               quiet_hours_start, quiet_hours_end, rate_limit_per_hour, last_fired_at
        FROM notifications
        ORDER BY created_at DESC
        "#,
    )
    .fetch_all(pool)
    .await?;
    let mut channels: Vec<Notification> = rows
        .into_iter()
        .map(|r| Notification {
            id: NotificationId::from_uuid(r.id),
            kind: r.kind,
            name: r.name,
            config: crate::secrets::open(r.config),
            active: r.active,
            template_id: r.template_id.map(NotificationTemplateId::from_uuid),
            created_at: r.created_at,
            cooldown_seconds: r.cooldown_seconds,
            digest_window_secs: r.digest_window_secs,
            quiet_hours_start: r.quiet_hours_start,
            quiet_hours_end: r.quiet_hours_end,
            rate_limit_per_hour: r.rate_limit_per_hour,
            last_fired_at: r.last_fired_at,
            tags: Vec::new(),
        })
        .collect();
    let ids: Vec<NotificationId> = channels.iter().map(|c| c.id).collect();
    let mut tag_map = crate::tags::hydrate_for_channels(pool, &ids).await?;
    for c in channels.iter_mut() {
        if let Some(t) = tag_map.remove(&c.id) {
            c.tags = t;
        }
    }
    Ok(channels)
}

/// Fetch one channel scoped to an org (cross-org id → NotFound). Request path.
pub async fn get(pool: &DbPool, id: NotificationId, org_id: OrgId) -> DbResult<Notification> {
    let row = sqlx::query!(
        r#"
        SELECT id, kind AS "kind: ChannelKind", name, config, active,
               template_id, created_at, cooldown_seconds, digest_window_secs,
               quiet_hours_start, quiet_hours_end, rate_limit_per_hour, last_fired_at
        FROM notifications
        WHERE id = $1 AND org_id = $2
        "#,
        id.0,
        org_id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    let nid = NotificationId::from_uuid(row.id);
    let mut tag_map = crate::tags::hydrate_for_channels(pool, &[nid]).await?;
    Ok(Notification {
        id: nid,
        kind: row.kind,
        name: row.name,
        config: crate::secrets::open(row.config),
        active: row.active,
        template_id: row.template_id.map(NotificationTemplateId::from_uuid),
        created_at: row.created_at,
        cooldown_seconds: row.cooldown_seconds,
        digest_window_secs: row.digest_window_secs,
        quiet_hours_start: row.quiet_hours_start,
        quiet_hours_end: row.quiet_hours_end,
        rate_limit_per_hour: row.rate_limit_per_hour,
        last_fired_at: row.last_fired_at,
        tags: tag_map.remove(&nid).unwrap_or_default(),
    })
}

/// Fetch one channel WITHOUT org scoping — for the notifier resolving a channel
/// to dispatch through (no request context). Not for request paths.
pub async fn get_unscoped(pool: &DbPool, id: NotificationId) -> DbResult<Notification> {
    let row = sqlx::query!(
        r#"
        SELECT id, kind AS "kind: ChannelKind", name, config, active,
               template_id, created_at, cooldown_seconds, digest_window_secs,
               quiet_hours_start, quiet_hours_end, rate_limit_per_hour, last_fired_at
        FROM notifications
        WHERE id = $1
        "#,
        id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    let nid = NotificationId::from_uuid(row.id);
    let mut tag_map = crate::tags::hydrate_for_channels(pool, &[nid]).await?;
    Ok(Notification {
        id: nid,
        kind: row.kind,
        name: row.name,
        config: crate::secrets::open(row.config),
        active: row.active,
        template_id: row.template_id.map(NotificationTemplateId::from_uuid),
        created_at: row.created_at,
        cooldown_seconds: row.cooldown_seconds,
        digest_window_secs: row.digest_window_secs,
        quiet_hours_start: row.quiet_hours_start,
        quiet_hours_end: row.quiet_hours_end,
        rate_limit_per_hour: row.rate_limit_per_hour,
        last_fired_at: row.last_fired_at,
        tags: tag_map.remove(&nid).unwrap_or_default(),
    })
}

pub async fn create(pool: &DbPool, input: NewNotification) -> DbResult<Notification> {
    let id = Uuid::now_v7();
    // Encrypt the credential-bearing config at rest (no-op without a key).
    let config_sealed = crate::secrets::seal(&input.config);
    let row = sqlx::query!(
        r#"
        INSERT INTO notifications (id, kind, name, config, active, template_id, cooldown_seconds, digest_window_secs,
                                   quiet_hours_start, quiet_hours_end, rate_limit_per_hour)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        RETURNING id, kind AS "kind: ChannelKind", name, config, active,
                  template_id, created_at, cooldown_seconds, digest_window_secs,
                  quiet_hours_start, quiet_hours_end, rate_limit_per_hour, last_fired_at
        "#,
        id,
        input.kind as ChannelKind,
        input.name,
        config_sealed,
        input.active,
        input.template_id.map(|t| t.0),
        input.cooldown_seconds,
        clamp_digest_window(input.digest_window_secs),
        clamp_hour(input.quiet_hours_start),
        clamp_hour(input.quiet_hours_end),
        clamp_rate(input.rate_limit_per_hour),
    )
    .fetch_one(pool)
    .await?;
    Ok(Notification {
        id: NotificationId::from_uuid(row.id),
        kind: row.kind,
        name: row.name,
        config: crate::secrets::open(row.config),
        active: row.active,
        template_id: row.template_id.map(NotificationTemplateId::from_uuid),
        created_at: row.created_at,
        cooldown_seconds: row.cooldown_seconds,
        digest_window_secs: row.digest_window_secs,
        quiet_hours_start: row.quiet_hours_start,
        quiet_hours_end: row.quiet_hours_end,
        rate_limit_per_hour: row.rate_limit_per_hour,
        last_fired_at: row.last_fired_at,
        tags: Vec::new(),
    })
}

pub async fn update(
    pool: &DbPool,
    id: NotificationId,
    input: UpdateNotification,
    org_id: OrgId,
) -> DbResult<Notification> {
    let cur = get(pool, id, org_id).await?;
    let new_name = input.name.unwrap_or(cur.name);
    let new_config = input.config.unwrap_or(cur.config);
    let new_active = input.active.unwrap_or(cur.active);
    let new_cooldown = input.cooldown_seconds.unwrap_or(cur.cooldown_seconds);
    let new_digest =
        clamp_digest_window(input.digest_window_secs.unwrap_or(cur.digest_window_secs));
    // template_id: outer None = keep current; outer Some(None) = clear;
    // outer Some(Some(id)) = set.
    let new_template_id = match input.template_id {
        None => cur.template_id.map(|t| t.0),
        Some(None) => None,
        Some(Some(t)) => Some(t.0),
    };
    // quiet-hours: same absent / explicit-null / set semantics.
    let new_quiet_start = clamp_hour(match input.quiet_hours_start {
        None => cur.quiet_hours_start,
        Some(v) => v,
    });
    let new_quiet_end = clamp_hour(match input.quiet_hours_end {
        None => cur.quiet_hours_end,
        Some(v) => v,
    });
    let new_rate = clamp_rate(input.rate_limit_per_hour.unwrap_or(cur.rate_limit_per_hour));
    let config_sealed = crate::secrets::seal(&new_config);

    let row = sqlx::query!(
        r#"
        UPDATE notifications
        SET name = $2, config = $3, active = $4, template_id = $5, cooldown_seconds = $6,
            digest_window_secs = $7, quiet_hours_start = $8, quiet_hours_end = $9,
            rate_limit_per_hour = $10
        WHERE id = $1 AND org_id = $11
        RETURNING id, kind AS "kind: ChannelKind", name, config, active,
                  template_id, created_at, cooldown_seconds, digest_window_secs,
                  quiet_hours_start, quiet_hours_end, rate_limit_per_hour, last_fired_at
        "#,
        id.0,
        new_name,
        config_sealed,
        new_active,
        new_template_id,
        new_cooldown,
        new_digest,
        new_quiet_start,
        new_quiet_end,
        new_rate,
        org_id.0,
    )
    .fetch_one(pool)
    .await?;
    Ok(Notification {
        id: NotificationId::from_uuid(row.id),
        kind: row.kind,
        name: row.name,
        config: crate::secrets::open(row.config),
        active: row.active,
        template_id: row.template_id.map(NotificationTemplateId::from_uuid),
        created_at: row.created_at,
        cooldown_seconds: row.cooldown_seconds,
        digest_window_secs: row.digest_window_secs,
        quiet_hours_start: row.quiet_hours_start,
        quiet_hours_end: row.quiet_hours_end,
        rate_limit_per_hour: row.rate_limit_per_hour,
        last_fired_at: row.last_fired_at,
        tags: Vec::new(),
    })
}

/// Count of attached, enabled channels per monitor — used by the dashboard
/// to render the bell badge without firing N requests.
#[derive(Debug, Clone, Serialize)]
pub struct MonitorChannelCount {
    pub monitor_id: MonitorId,
    pub count: i64,
}

pub async fn counts_per_monitor(
    pool: &DbPool,
    org_id: OrgId,
) -> DbResult<Vec<MonitorChannelCount>> {
    let rows = sqlx::query!(
        r#"
        SELECT mn.monitor_id, COUNT(*)::int8 AS "count!"
        FROM monitor_notifications mn
        JOIN notifications n ON n.id = mn.notification_id
        WHERE n.active AND n.org_id = $1
        GROUP BY mn.monitor_id
        "#,
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| MonitorChannelCount {
            monitor_id: MonitorId::from_uuid(r.monitor_id),
            count: r.count,
        })
        .collect())
}

pub async fn delete(pool: &DbPool, id: NotificationId, org_id: OrgId) -> DbResult<()> {
    let r = sqlx::query!(
        r#"DELETE FROM notifications WHERE id = $1 AND org_id = $2"#,
        id.0,
        org_id.0,
    )
    .execute(pool)
    .await?;
    if r.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

pub async fn attach(pool: &DbPool, monitor: MonitorId, notif: NotificationId) -> DbResult<()> {
    sqlx::query!(
        r#"
        INSERT INTO monitor_notifications (monitor_id, notification_id)
        VALUES ($1, $2)
        ON CONFLICT DO NOTHING
        "#,
        monitor.0,
        notif.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn detach(pool: &DbPool, monitor: MonitorId, notif: NotificationId) -> DbResult<()> {
    sqlx::query!(
        r#"DELETE FROM monitor_notifications WHERE monitor_id = $1 AND notification_id = $2"#,
        monitor.0,
        notif.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn for_monitor(pool: &DbPool, monitor: MonitorId) -> DbResult<Vec<Notification>> {
    let rows = sqlx::query!(
        r#"
        SELECT n.id, n.kind AS "kind: ChannelKind", n.name, n.config, n.active,
               n.template_id, n.created_at, n.cooldown_seconds, n.digest_window_secs,
               n.quiet_hours_start, n.quiet_hours_end, n.rate_limit_per_hour, n.last_fired_at
        FROM notifications n
        JOIN monitor_notifications mn ON mn.notification_id = n.id
        WHERE mn.monitor_id = $1 AND n.active
        "#,
        monitor.0,
    )
    .fetch_all(pool)
    .await?;
    let mut channels: Vec<Notification> = rows
        .into_iter()
        .map(|r| Notification {
            id: NotificationId::from_uuid(r.id),
            kind: r.kind,
            name: r.name,
            config: crate::secrets::open(r.config),
            active: r.active,
            template_id: r.template_id.map(NotificationTemplateId::from_uuid),
            created_at: r.created_at,
            cooldown_seconds: r.cooldown_seconds,
            digest_window_secs: r.digest_window_secs,
            quiet_hours_start: r.quiet_hours_start,
            quiet_hours_end: r.quiet_hours_end,
            rate_limit_per_hour: r.rate_limit_per_hour,
            last_fired_at: r.last_fired_at,
            tags: Vec::new(),
        })
        .collect();
    let ids: Vec<NotificationId> = channels.iter().map(|c| c.id).collect();
    let mut tag_map = crate::tags::hydrate_for_channels(pool, &ids).await?;
    for c in channels.iter_mut() {
        if let Some(t) = tag_map.remove(&c.id) {
            c.tags = t;
        }
    }
    Ok(channels)
}

/// Stamp last_fired_at = NOW() after a successful send.
pub async fn mark_fired(pool: &DbPool, id: NotificationId) -> DbResult<()> {
    sqlx::query!(
        "UPDATE notifications SET last_fired_at = NOW() WHERE id = $1",
        id.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}
