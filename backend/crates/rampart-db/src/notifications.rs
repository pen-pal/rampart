//! Notification channel queries.

use crate::{DbError, DbPool, DbResult};
use rampart_core::ids::{MonitorId, NotificationId, NotificationTemplateId};
use rampart_core::ChannelKind;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct Notification {
    pub id:          NotificationId,
    pub kind:        ChannelKind,
    pub name:        String,
    pub config:      serde_json::Value,
    pub active:      bool,
    pub template_id: Option<NotificationTemplateId>,
    pub created_at:  OffsetDateTime,
}

#[derive(Debug, Deserialize)]
pub struct NewNotification {
    pub kind:    ChannelKind,
    pub name:    String,
    pub config:  serde_json::Value,
    #[serde(default = "default_enabled")]
    pub active:  bool,
    #[serde(default)]
    pub template_id: Option<NotificationTemplateId>,
}
fn default_enabled() -> bool { true }

#[derive(Debug, Deserialize)]
pub struct UpdateNotification {
    #[serde(default)] pub name:        Option<String>,
    #[serde(default)] pub config:      Option<serde_json::Value>,
    #[serde(default)] pub active:      Option<bool>,
    /// Outer Option = "field present in payload". Inner Option = "explicit null
    /// → clear the assignment". Set None to leave unchanged.
    #[serde(default)] pub template_id: Option<Option<NotificationTemplateId>>,
}

pub async fn list(pool: &DbPool) -> DbResult<Vec<Notification>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, kind AS "kind: ChannelKind", name, config, active,
               template_id, created_at
        FROM notifications
        ORDER BY created_at DESC
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| Notification {
            id:          NotificationId::from_uuid(r.id),
            kind:        r.kind,
            name:        r.name,
            config:      r.config,
            active:      r.active,
            template_id: r.template_id.map(NotificationTemplateId::from_uuid),
            created_at:  r.created_at,
        })
        .collect())
}

pub async fn get(pool: &DbPool, id: NotificationId) -> DbResult<Notification> {
    let row = sqlx::query!(
        r#"
        SELECT id, kind AS "kind: ChannelKind", name, config, active,
               template_id, created_at
        FROM notifications
        WHERE id = $1
        "#,
        id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(Notification {
        id:          NotificationId::from_uuid(row.id),
        kind:        row.kind,
        name:        row.name,
        config:      row.config,
        active:      row.active,
        template_id: row.template_id.map(NotificationTemplateId::from_uuid),
        created_at:  row.created_at,
    })
}

pub async fn create(pool: &DbPool, input: NewNotification) -> DbResult<Notification> {
    let id = Uuid::now_v7();
    let row = sqlx::query!(
        r#"
        INSERT INTO notifications (id, kind, name, config, active, template_id)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id, kind AS "kind: ChannelKind", name, config, active,
                  template_id, created_at
        "#,
        id,
        input.kind as ChannelKind,
        input.name,
        input.config,
        input.active,
        input.template_id.map(|t| t.0),
    )
    .fetch_one(pool)
    .await?;
    Ok(Notification {
        id:          NotificationId::from_uuid(row.id),
        kind:        row.kind,
        name:        row.name,
        config:      row.config,
        active:      row.active,
        template_id: row.template_id.map(NotificationTemplateId::from_uuid),
        created_at:  row.created_at,
    })
}

pub async fn update(pool: &DbPool, id: NotificationId, input: UpdateNotification) -> DbResult<Notification> {
    let cur = get(pool, id).await?;
    let new_name   = input.name.unwrap_or(cur.name);
    let new_config = input.config.unwrap_or(cur.config);
    let new_active = input.active.unwrap_or(cur.active);
    // template_id: outer None = keep current; outer Some(None) = clear;
    // outer Some(Some(id)) = set.
    let new_template_id = match input.template_id {
        None         => cur.template_id.map(|t| t.0),
        Some(None)   => None,
        Some(Some(t)) => Some(t.0),
    };

    let row = sqlx::query!(
        r#"
        UPDATE notifications
        SET name = $2, config = $3, active = $4, template_id = $5
        WHERE id = $1
        RETURNING id, kind AS "kind: ChannelKind", name, config, active,
                  template_id, created_at
        "#,
        id.0, new_name, new_config, new_active, new_template_id,
    )
    .fetch_one(pool).await?;
    Ok(Notification {
        id:          NotificationId::from_uuid(row.id),
        kind:        row.kind,
        name:        row.name,
        config:      row.config,
        active:      row.active,
        template_id: row.template_id.map(NotificationTemplateId::from_uuid),
        created_at:  row.created_at,
    })
}

/// Count of attached, enabled channels per monitor — used by the dashboard
/// to render the bell badge without firing N requests.
#[derive(Debug, Clone, Serialize)]
pub struct MonitorChannelCount {
    pub monitor_id: MonitorId,
    pub count:      i64,
}

pub async fn counts_per_monitor(pool: &DbPool) -> DbResult<Vec<MonitorChannelCount>> {
    let rows = sqlx::query!(
        r#"
        SELECT mn.monitor_id, COUNT(*)::int8 AS "count!"
        FROM monitor_notifications mn
        JOIN notifications n ON n.id = mn.notification_id
        WHERE n.active
        GROUP BY mn.monitor_id
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| MonitorChannelCount {
        monitor_id: MonitorId::from_uuid(r.monitor_id),
        count:      r.count,
    }).collect())
}

pub async fn delete(pool: &DbPool, id: NotificationId) -> DbResult<()> {
    let r = sqlx::query!(r#"DELETE FROM notifications WHERE id = $1"#, id.0)
        .execute(pool).await?;
    if r.rows_affected() == 0 { return Err(DbError::NotFound); }
    Ok(())
}

pub async fn attach(pool: &DbPool, monitor: MonitorId, notif: NotificationId) -> DbResult<()> {
    sqlx::query!(
        r#"
        INSERT INTO monitor_notifications (monitor_id, notification_id)
        VALUES ($1, $2)
        ON CONFLICT DO NOTHING
        "#,
        monitor.0, notif.0,
    ).execute(pool).await?;
    Ok(())
}

pub async fn detach(pool: &DbPool, monitor: MonitorId, notif: NotificationId) -> DbResult<()> {
    sqlx::query!(
        r#"DELETE FROM monitor_notifications WHERE monitor_id = $1 AND notification_id = $2"#,
        monitor.0, notif.0,
    ).execute(pool).await?;
    Ok(())
}

pub async fn for_monitor(pool: &DbPool, monitor: MonitorId) -> DbResult<Vec<Notification>> {
    let rows = sqlx::query!(
        r#"
        SELECT n.id, n.kind AS "kind: ChannelKind", n.name, n.config, n.active,
               n.template_id, n.created_at
        FROM notifications n
        JOIN monitor_notifications mn ON mn.notification_id = n.id
        WHERE mn.monitor_id = $1 AND n.active
        "#,
        monitor.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| Notification {
            id:          NotificationId::from_uuid(r.id),
            kind:        r.kind,
            name:        r.name,
            config:      r.config,
            active:      r.active,
            template_id: r.template_id.map(NotificationTemplateId::from_uuid),
            created_at:  r.created_at,
        })
        .collect())
}
