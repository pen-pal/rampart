//! Monitor-preset repository.
//!
//! A tiny global library of reusable monitor-config slices (saved HTTP
//! header sets, saved TLS posture) the New-Monitor wizard applies with a
//! click. `kind` is stored as TEXT pinned by a DB CHECK; `data` is loose
//! JSONB whose shape the wizard owns. Global, not monitor-scoped — the
//! whole point is reuse across monitors.

use crate::{DbError, DbPool, DbResult};
use rampart_core::ids::OrgId;
use rampart_core::monitor_preset::{MonitorPreset, MonitorPresetKind, NewMonitorPreset};
use rampart_core::MonitorPresetId;

/// Map a stored `kind` string onto the enum. An unknown value means the
/// CHECK constraint was bypassed (shouldn't happen) — surface it as a
/// data error rather than silently defaulting.
fn parse_kind(s: &str) -> DbResult<MonitorPresetKind> {
    MonitorPresetKind::from_db_str(s)
        .ok_or_else(|| DbError::Conflict(format!("unknown monitor preset kind: {s}")))
}

pub async fn list(pool: &DbPool, org_id: OrgId) -> DbResult<Vec<MonitorPreset>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, name, kind, data, created_at
        FROM monitor_presets
        WHERE org_id = $1
        ORDER BY created_at DESC
        "#,
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|r| {
            Ok(MonitorPreset {
                id: MonitorPresetId::from_uuid(r.id),
                name: r.name,
                kind: parse_kind(&r.kind)?,
                data: r.data,
                created_at: r.created_at,
            })
        })
        .collect()
}

pub async fn get(pool: &DbPool, id: MonitorPresetId, org_id: OrgId) -> DbResult<MonitorPreset> {
    let r = sqlx::query!(
        r#"
        SELECT id, name, kind, data, created_at
        FROM monitor_presets
        WHERE id = $1 AND org_id = $2
        "#,
        id.0,
        org_id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(MonitorPreset {
        id: MonitorPresetId::from_uuid(r.id),
        name: r.name,
        kind: parse_kind(&r.kind)?,
        data: r.data,
        created_at: r.created_at,
    })
}

pub async fn create(
    pool: &DbPool,
    input: NewMonitorPreset,
    org_id: rampart_core::ids::OrgId,
) -> DbResult<MonitorPreset> {
    let id = MonitorPresetId::new();
    // Default the bag to an empty object when the caller sent JSON null —
    // the column is NOT NULL.
    let data = if input.data.is_null() {
        serde_json::json!({})
    } else {
        input.data
    };
    let r = sqlx::query!(
        r#"
        INSERT INTO monitor_presets (id, name, kind, data, org_id)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, name, kind, data, created_at
        "#,
        id.0,
        input.name,
        input.kind.as_str(),
        data,
        org_id.0,
    )
    .fetch_one(pool)
    .await?;
    Ok(MonitorPreset {
        id: MonitorPresetId::from_uuid(r.id),
        name: r.name,
        kind: parse_kind(&r.kind)?,
        data: r.data,
        created_at: r.created_at,
    })
}

pub async fn delete(pool: &DbPool, id: MonitorPresetId, org_id: OrgId) -> DbResult<()> {
    let r = sqlx::query!(
        "DELETE FROM monitor_presets WHERE id = $1 AND org_id = $2",
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
