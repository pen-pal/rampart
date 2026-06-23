//! MySQL `monitor_presets` domain — reusable monitor-config slices (saved HTTP
//! header sets / TLS posture) the New-Monitor wizard applies. Ported from PG.
//! JSONB→LONGTEXT; `kind` CHECK ported; no `RETURNING` → INSERT-then-re-select.

use super::{raw_uuid, ts};
use crate::{DbError, DbResult};
use rampart_core::ids::OrgId;
use rampart_core::monitor_preset::{MonitorPreset, MonitorPresetKind, NewMonitorPreset};
use rampart_core::MonitorPresetId;
use sqlx::{MySqlPool, Row};

fn parse_kind(s: &str) -> DbResult<MonitorPresetKind> {
    MonitorPresetKind::from_db_str(s)
        .ok_or_else(|| DbError::Conflict(format!("unknown monitor preset kind: {s}")))
}

fn preset_from(r: &sqlx::mysql::MySqlRow) -> DbResult<MonitorPreset> {
    Ok(MonitorPreset {
        id: MonitorPresetId::from_uuid(raw_uuid(&r.get::<String, _>("id"))),
        name: r.get("name"),
        kind: parse_kind(&r.get::<String, _>("kind"))?,
        data: serde_json::from_str(&r.get::<String, _>("data"))
            .unwrap_or_else(|_| serde_json::json!({})),
        created_at: ts(r.get::<i64, _>("created_at")),
    })
}

const COLS: &str = "id, name, kind, data, created_at";

pub async fn list(pool: &MySqlPool, org_id: OrgId) -> DbResult<Vec<MonitorPreset>> {
    let sql =
        format!("SELECT {COLS} FROM monitor_presets WHERE org_id = ? ORDER BY created_at DESC");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(org_id.0.to_string())
        .fetch_all(pool)
        .await?;
    rows.iter().map(preset_from).collect()
}

pub async fn get(pool: &MySqlPool, id: MonitorPresetId, org_id: OrgId) -> DbResult<MonitorPreset> {
    let sql = format!("SELECT {COLS} FROM monitor_presets WHERE id = ? AND org_id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound)?;
    preset_from(&row)
}

pub async fn create(
    pool: &MySqlPool,
    input: NewMonitorPreset,
    org_id: OrgId,
) -> DbResult<MonitorPreset> {
    let id = MonitorPresetId::new();
    let data = if input.data.is_null() {
        serde_json::json!({})
    } else {
        input.data
    };
    sqlx::query(
        "INSERT INTO monitor_presets (id, name, kind, data, org_id) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(id.0.to_string())
    .bind(input.name)
    .bind(input.kind.as_str())
    .bind(serde_json::to_string(&data).unwrap_or_else(|_| "{}".into()))
    .bind(org_id.0.to_string())
    .execute(pool)
    .await?;
    get(pool, id, org_id).await
}

pub async fn delete(pool: &MySqlPool, id: MonitorPresetId, org_id: OrgId) -> DbResult<()> {
    let r = sqlx::query("DELETE FROM monitor_presets WHERE id = ? AND org_id = ?")
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .execute(pool)
        .await?;
    if r.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn crud(pool: MySqlPool) {
        let org = super::super::oid(DEF);
        let p = create(
            &pool,
            NewMonitorPreset {
                name: "prod headers".into(),
                kind: MonitorPresetKind::HttpHeaders,
                data: serde_json::json!({ "Authorization": "Bearer x" }),
            },
            org,
        )
        .await
        .unwrap();
        assert_eq!(p.kind, MonitorPresetKind::HttpHeaders);
        assert_eq!(p.data["Authorization"], "Bearer x");
        assert_eq!(list(&pool, org).await.unwrap().len(), 1);
        assert_eq!(get(&pool, p.id, org).await.unwrap().name, "prod headers");

        // cross-org isolation.
        let other = super::super::orgs::create(&pool, "other", "Other")
            .await
            .unwrap();
        assert!(matches!(
            get(&pool, p.id, other.id).await,
            Err(DbError::NotFound)
        ));

        delete(&pool, p.id, org).await.unwrap();
        assert!(matches!(
            delete(&pool, p.id, org).await,
            Err(DbError::NotFound)
        ));
    }
}
