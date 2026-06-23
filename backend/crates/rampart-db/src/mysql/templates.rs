//! MySQL `templates` domain — the dispatch-path read `get_render_strings`
//! (the notifier renders a channel's custom subject/body). CRUD stays stubbed
//! until a management-API slice needs it. The `notification_templates` table
//! already exists (migrations-mysql/0006_notifications.sql).

use crate::templates::RenderedTemplate;
use crate::{DbError, DbResult};
use rampart_core::ids::NotificationTemplateId;
use sqlx::{MySqlPool, Row};

pub async fn get_render_strings(
    pool: &MySqlPool,
    id: NotificationTemplateId,
) -> DbResult<RenderedTemplate> {
    let row = sqlx::query(
        "SELECT subject_template, body_template FROM notification_templates WHERE id = ?",
    )
    .bind(id.0.to_string())
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(RenderedTemplate {
        subject: row.get::<Option<String>, _>("subject_template"),
        body: row.get::<String, _>("body_template"),
    })
}
