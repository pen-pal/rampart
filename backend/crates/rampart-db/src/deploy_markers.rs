//! Deploy-marker repository: create + list-in-window + delete. See
//! `rampart_core::deploy_marker` for the type.

use crate::{DbError, DbPool, DbResult};
use rampart_core::deploy_marker::{DeployMarker, NewDeployMarker};
use rampart_core::ids::DeployMarkerId;
use time::OffsetDateTime;
use uuid::Uuid;

struct MarkerRow {
    id: Uuid,
    ts: OffsetDateTime,
    title: String,
    description: Option<String>,
    service: Option<String>,
    created_at: OffsetDateTime,
}

impl From<MarkerRow> for DeployMarker {
    fn from(r: MarkerRow) -> Self {
        DeployMarker {
            id: DeployMarkerId::from_uuid(r.id),
            ts: r.ts,
            title: r.title,
            description: r.description,
            service: r.service,
            created_at: r.created_at,
        }
    }
}

pub async fn create(
    pool: &DbPool,
    input: NewDeployMarker,
    org_id: rampart_core::ids::OrgId,
) -> DbResult<DeployMarker> {
    let id = DeployMarkerId::new();
    let row = sqlx::query_as!(
        MarkerRow,
        r#"
        INSERT INTO deploy_markers (id, ts, title, description, service, org_id)
        VALUES ($1, COALESCE($2, now()), $3, $4, $5, $6)
        RETURNING id, ts, title, description, service, created_at
        "#,
        id.0,
        input.ts,
        input.title,
        input.description,
        input.service,
        org_id.0,
    )
    .fetch_one(pool)
    .await?;
    Ok(row.into())
}

/// Markers within the trailing `hours`, newest first. Optionally scoped to a
/// service (returns service-scoped + global/unscoped markers). Org-scoped so
/// one tenant's deploy timeline never bleeds onto another's charts.
pub async fn list_window(
    pool: &DbPool,
    hours: i32,
    service: Option<&str>,
    org_id: rampart_core::ids::OrgId,
) -> DbResult<Vec<DeployMarker>> {
    let rows = sqlx::query_as!(
        MarkerRow,
        r#"
        SELECT id, ts, title, description, service, created_at
        FROM deploy_markers
        WHERE ts > now() - make_interval(hours => $1)
          AND ($2::text IS NULL OR service IS NULL OR service = $2)
          AND org_id = $3
        ORDER BY ts DESC
        LIMIT 500
        "#,
        hours.clamp(1, 8760),
        service,
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn delete(
    pool: &DbPool,
    id: DeployMarkerId,
    org_id: rampart_core::ids::OrgId,
) -> DbResult<()> {
    let res = sqlx::query!(
        "DELETE FROM deploy_markers WHERE id = $1 AND org_id = $2",
        id.0,
        org_id.0,
    )
    .execute(pool)
    .await?;
    if res.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}
