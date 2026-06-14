//! Source-map storage (migration 0086) for error-tier symbolication.
//!
//! A map is keyed by `(project_id, release, filename)` where `filename` is the
//! **basename** of the minified file a stack frame references. On read, the API
//! looks up the map for an event's release + each frame's file, and resolves
//! `(line, col)` to the original function/file/line with the `sourcemap` crate.
//! See `docs/design/ERROR-TRACKING.md`.

use crate::{DbPool, DbResult};
use serde::Serialize;
use time::OffsetDateTime;
use uuid::Uuid;

pub struct NewSourceMap<'a> {
    pub project_id: Uuid,
    pub release: &'a str,
    pub filename: &'a str,
    pub map: serde_json::Value,
}

/// Insert or replace the map for a `(project, release, filename)`.
pub async fn upsert(pool: &DbPool, m: NewSourceMap<'_>) -> DbResult<i64> {
    let row = sqlx::query!(
        r#"
        INSERT INTO source_maps (project_id, release, filename, map)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (project_id, release, filename)
        DO UPDATE SET map = EXCLUDED.map, uploaded_at = now()
        RETURNING id
        "#,
        m.project_id,
        m.release,
        m.filename,
        m.map,
    )
    .fetch_one(pool)
    .await?;
    Ok(row.id)
}

/// The map document for resolution, or None if none uploaded.
pub async fn get(
    pool: &DbPool,
    project_id: Uuid,
    release: &str,
    filename: &str,
) -> DbResult<Option<serde_json::Value>> {
    let row = sqlx::query!(
        r#"SELECT map AS "map!: serde_json::Value" FROM source_maps
           WHERE project_id = $1 AND release = $2 AND filename = $3"#,
        project_id,
        release,
        filename,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| r.map))
}

#[derive(Debug, Serialize)]
pub struct SourceMapMeta {
    pub id: i64,
    pub release: String,
    pub filename: String,
    pub uploaded_at: OffsetDateTime,
}

/// List uploaded maps for a project (metadata only — no map bodies).
pub async fn list(pool: &DbPool, project_id: Uuid) -> DbResult<Vec<SourceMapMeta>> {
    let rows = sqlx::query_as!(
        SourceMapMeta,
        r#"SELECT id, release, filename, uploaded_at
           FROM source_maps WHERE project_id = $1
           ORDER BY release, filename"#,
        project_id,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Delete one map (scoped to the project for safety). Returns whether a row went.
pub async fn delete(pool: &DbPool, project_id: Uuid, id: i64) -> DbResult<bool> {
    let r = sqlx::query!(
        "DELETE FROM source_maps WHERE id = $1 AND project_id = $2",
        id,
        project_id,
    )
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}
