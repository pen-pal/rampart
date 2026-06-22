//! Incident-update template queries.
//!
//! A tiny global library of canned incident bodies + styles. Operators
//! repeat the same phases ("Investigating", "Identified", "Monitoring",
//! "Resolved") on every incident; a template lets them apply one instead
//! of retyping. Global, not page-scoped — same phrasing everywhere.

use crate::{DbError, DbPool, DbResult};
use rampart_core::ids::OrgId;
use rampart_core::{IncidentStyle, IncidentTemplate, IncidentTemplateId};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct NewIncidentTemplate {
    pub name: String,
    pub body: String,
    #[serde(default = "default_style")]
    pub style: IncidentStyle,
}

fn default_style() -> IncidentStyle {
    IncidentStyle::Warning
}

#[derive(Debug, Deserialize)]
pub struct UpdateIncidentTemplate {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub style: Option<IncidentStyle>,
}

pub async fn list(pool: &DbPool, org_id: OrgId) -> DbResult<Vec<IncidentTemplate>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, name, body, style AS "style: IncidentStyle", created_at
        FROM incident_templates
        WHERE org_id = $1
        ORDER BY created_at DESC
        "#,
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| IncidentTemplate {
            id: IncidentTemplateId::from_uuid(r.id),
            name: r.name,
            body: r.body,
            style: r.style,
            created_at: r.created_at,
        })
        .collect())
}

pub async fn get(
    pool: &DbPool,
    id: IncidentTemplateId,
    org_id: OrgId,
) -> DbResult<IncidentTemplate> {
    let r = sqlx::query!(
        r#"
        SELECT id, name, body, style AS "style: IncidentStyle", created_at
        FROM incident_templates
        WHERE id = $1 AND org_id = $2
        "#,
        id.0,
        org_id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(IncidentTemplate {
        id: IncidentTemplateId::from_uuid(r.id),
        name: r.name,
        body: r.body,
        style: r.style,
        created_at: r.created_at,
    })
}

pub async fn create(
    pool: &DbPool,
    input: NewIncidentTemplate,
    org_id: rampart_core::ids::OrgId,
) -> DbResult<IncidentTemplate> {
    let id = Uuid::now_v7();
    let r = sqlx::query!(
        r#"
        INSERT INTO incident_templates (id, name, body, style, org_id)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, name, body, style AS "style: IncidentStyle", created_at
        "#,
        id,
        input.name,
        input.body,
        input.style as IncidentStyle,
        org_id.0,
    )
    .fetch_one(pool)
    .await?;
    Ok(IncidentTemplate {
        id: IncidentTemplateId::from_uuid(r.id),
        name: r.name,
        body: r.body,
        style: r.style,
        created_at: r.created_at,
    })
}

pub async fn update(
    pool: &DbPool,
    id: IncidentTemplateId,
    input: UpdateIncidentTemplate,
    org_id: OrgId,
) -> DbResult<IncidentTemplate> {
    let cur = get(pool, id, org_id).await?;
    let r = sqlx::query!(
        r#"
        UPDATE incident_templates
        SET name  = $2,
            body  = $3,
            style = $4
        WHERE id = $1 AND org_id = $5
        RETURNING id, name, body, style AS "style: IncidentStyle", created_at
        "#,
        id.0,
        input.name.unwrap_or(cur.name),
        input.body.unwrap_or(cur.body),
        input.style.unwrap_or(cur.style) as IncidentStyle,
        org_id.0,
    )
    .fetch_one(pool)
    .await?;
    Ok(IncidentTemplate {
        id: IncidentTemplateId::from_uuid(r.id),
        name: r.name,
        body: r.body,
        style: r.style,
        created_at: r.created_at,
    })
}

pub async fn delete(pool: &DbPool, id: IncidentTemplateId, org_id: OrgId) -> DbResult<()> {
    let r = sqlx::query!(
        "DELETE FROM incident_templates WHERE id = $1 AND org_id = $2",
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
