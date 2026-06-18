//! Monitor-template repository.
//!
//! A template is a named, reusable WHOLE monitor spec — distinct from a
//! preset (a small header/TLS config slice). `spec` is the freeform
//! monitor-creation body (a `NewMonitor` JSON object) stored verbatim as
//! JSONB so it can be fed straight back through the monitor-create path on
//! instantiate. Global, not monitor-scoped — reuse across monitors is the
//! whole point.

use crate::{DbError, DbPool, DbResult};
use rampart_core::ids::OrgId;
use rampart_core::MonitorTemplateId;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// A stored template row as returned to the API tier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorTemplate {
    pub id: MonitorTemplateId,
    pub name: String,
    pub description: Option<String>,
    /// The freeform monitor-creation body (a `NewMonitor` JSON object).
    pub spec: serde_json::Value,
    pub created_at: OffsetDateTime,
}

/// Input for creating a template.
#[derive(Debug, Clone, Deserialize)]
pub struct NewMonitorTemplate {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub spec: serde_json::Value,
}

pub async fn list(pool: &DbPool, org_id: OrgId) -> DbResult<Vec<MonitorTemplate>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, name, description, spec, created_at
        FROM monitor_templates
        WHERE org_id = $1
        ORDER BY created_at DESC
        "#,
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| MonitorTemplate {
            id: MonitorTemplateId::from_uuid(r.id),
            name: r.name,
            description: r.description,
            spec: r.spec,
            created_at: r.created_at,
        })
        .collect())
}

pub async fn get(pool: &DbPool, id: MonitorTemplateId, org_id: OrgId) -> DbResult<MonitorTemplate> {
    let r = sqlx::query!(
        r#"
        SELECT id, name, description, spec, created_at
        FROM monitor_templates
        WHERE id = $1 AND org_id = $2
        "#,
        id.0,
        org_id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(MonitorTemplate {
        id: MonitorTemplateId::from_uuid(r.id),
        name: r.name,
        description: r.description,
        spec: r.spec,
        created_at: r.created_at,
    })
}

pub async fn create(
    pool: &DbPool,
    input: NewMonitorTemplate,
    org_id: rampart_core::ids::OrgId,
) -> DbResult<MonitorTemplate> {
    let id = MonitorTemplateId::new();
    let r = sqlx::query!(
        r#"
        INSERT INTO monitor_templates (id, name, description, spec, org_id)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, name, description, spec, created_at
        "#,
        id.0,
        input.name,
        input.description,
        input.spec,
        org_id.0,
    )
    .fetch_one(pool)
    .await?;
    Ok(MonitorTemplate {
        id: MonitorTemplateId::from_uuid(r.id),
        name: r.name,
        description: r.description,
        spec: r.spec,
        created_at: r.created_at,
    })
}

pub async fn delete(pool: &DbPool, id: MonitorTemplateId, org_id: OrgId) -> DbResult<()> {
    let r = sqlx::query!(
        "DELETE FROM monitor_templates WHERE id = $1 AND org_id = $2",
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
