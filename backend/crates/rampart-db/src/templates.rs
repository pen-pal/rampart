//! Notification template queries.
//!
//! Templates are reusable subject/body strings rendered through the same
//! `{{placeholder}}` engine the notifier uses for defaults. Channels can
//! reference one via `notifications.template_id`.

use crate::{DbError, DbPool, DbResult};
use rampart_core::ids::NotificationTemplateId;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct Template {
    pub id: NotificationTemplateId,
    pub name: String,
    pub channel_kinds: Vec<String>,
    pub event_kind: String,
    pub subject_template: Option<String>,
    pub body_template: String,
    pub is_default: bool,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Deserialize)]
pub struct NewTemplate {
    pub name: String,
    #[serde(default)]
    pub channel_kinds: Vec<String>,
    #[serde(default = "default_event_kind")]
    pub event_kind: String,
    #[serde(default)]
    pub subject_template: Option<String>,
    pub body_template: String,
    #[serde(default)]
    pub is_default: bool,
}
fn default_event_kind() -> String {
    "monitor_down".into()
}

#[derive(Debug, Deserialize)]
pub struct UpdateTemplate {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub channel_kinds: Option<Vec<String>>,
    #[serde(default)]
    pub event_kind: Option<String>,
    /// Use `Option<Option<String>>` so callers can explicitly clear the
    /// subject by sending `null`. See `double_option` for why a custom
    /// deserializer is needed.
    #[serde(default, deserialize_with = "double_option")]
    pub subject_template: Option<Option<String>>,
    #[serde(default)]
    pub body_template: Option<String>,
    #[serde(default)]
    pub is_default: Option<bool>,
}

fn double_option<'de, T, D>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    T: serde::Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    serde::Deserialize::deserialize(d).map(Some)
}

pub async fn list(pool: &DbPool) -> DbResult<Vec<Template>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, name, channel_kinds, event_kind, subject_template, body_template,
               is_default, created_at
        FROM notification_templates
        ORDER BY created_at DESC
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| Template {
            id: NotificationTemplateId::from_uuid(r.id),
            name: r.name,
            channel_kinds: r.channel_kinds,
            event_kind: r.event_kind,
            subject_template: r.subject_template,
            body_template: r.body_template,
            is_default: r.is_default,
            created_at: r.created_at,
        })
        .collect())
}

pub async fn get(pool: &DbPool, id: NotificationTemplateId) -> DbResult<Template> {
    let r = sqlx::query!(
        r#"
        SELECT id, name, channel_kinds, event_kind, subject_template, body_template,
               is_default, created_at
        FROM notification_templates
        WHERE id = $1
        "#,
        id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(Template {
        id: NotificationTemplateId::from_uuid(r.id),
        name: r.name,
        channel_kinds: r.channel_kinds,
        event_kind: r.event_kind,
        subject_template: r.subject_template,
        body_template: r.body_template,
        is_default: r.is_default,
        created_at: r.created_at,
    })
}

pub async fn create(pool: &DbPool, input: NewTemplate) -> DbResult<Template> {
    let id = Uuid::now_v7();
    let r = sqlx::query!(
        r#"
        INSERT INTO notification_templates
            (id, name, channel_kinds, event_kind, subject_template, body_template, is_default)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING id, name, channel_kinds, event_kind, subject_template, body_template,
                  is_default, created_at
        "#,
        id,
        input.name,
        &input.channel_kinds,
        input.event_kind,
        input.subject_template,
        input.body_template,
        input.is_default,
    )
    .fetch_one(pool)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(d) if d.is_unique_violation() => {
            DbError::Conflict("template name already exists".into())
        }
        _ => DbError::Sqlx(e),
    })?;
    Ok(Template {
        id: NotificationTemplateId::from_uuid(r.id),
        name: r.name,
        channel_kinds: r.channel_kinds,
        event_kind: r.event_kind,
        subject_template: r.subject_template,
        body_template: r.body_template,
        is_default: r.is_default,
        created_at: r.created_at,
    })
}

pub async fn update(
    pool: &DbPool,
    id: NotificationTemplateId,
    input: UpdateTemplate,
) -> DbResult<Template> {
    let cur = get(pool, id).await?;
    let r = sqlx::query!(
        r#"
        UPDATE notification_templates
        SET name             = $2,
            channel_kinds    = $3,
            event_kind       = $4,
            subject_template = $5,
            body_template    = $6,
            is_default       = $7
        WHERE id = $1
        RETURNING id, name, channel_kinds, event_kind, subject_template, body_template,
                  is_default, created_at
        "#,
        id.0,
        input.name.unwrap_or(cur.name),
        &input.channel_kinds.unwrap_or(cur.channel_kinds),
        input.event_kind.unwrap_or(cur.event_kind),
        input.subject_template.unwrap_or(cur.subject_template),
        input.body_template.unwrap_or(cur.body_template),
        input.is_default.unwrap_or(cur.is_default),
    )
    .fetch_one(pool)
    .await?;
    Ok(Template {
        id: NotificationTemplateId::from_uuid(r.id),
        name: r.name,
        channel_kinds: r.channel_kinds,
        event_kind: r.event_kind,
        subject_template: r.subject_template,
        body_template: r.body_template,
        is_default: r.is_default,
        created_at: r.created_at,
    })
}

pub async fn delete(pool: &DbPool, id: NotificationTemplateId) -> DbResult<()> {
    let r = sqlx::query!(r#"DELETE FROM notification_templates WHERE id = $1"#, id.0)
        .execute(pool)
        .await?;
    if r.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

/// Used by the notifier when a channel has a template_id assigned —
/// fetches just the strings needed for rendering, avoiding the heavier
/// `get` query when we don't need metadata.
#[derive(Debug, Clone)]
pub struct RenderedTemplate {
    pub subject: Option<String>,
    pub body: String,
}

pub async fn get_render_strings(
    pool: &DbPool,
    id: NotificationTemplateId,
) -> DbResult<RenderedTemplate> {
    let r = sqlx::query!(
        r#"SELECT subject_template, body_template FROM notification_templates WHERE id = $1"#,
        id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(RenderedTemplate {
        subject: r.subject_template,
        body: r.body_template,
    })
}
