//! MySQL `monitor_templates` domain — named reusable whole-monitor specs.
//! Ported from PG. JSONB spec→LONGTEXT; no `RETURNING` → INSERT-then-re-select.

use super::{raw_uuid, ts};
use crate::monitor_templates::{MonitorTemplate, NewMonitorTemplate};
use crate::{DbError, DbResult};
use rampart_core::ids::OrgId;
use rampart_core::MonitorTemplateId;
use sqlx::{MySqlPool, Row};

fn template_from(r: &sqlx::mysql::MySqlRow) -> MonitorTemplate {
    MonitorTemplate {
        id: MonitorTemplateId::from_uuid(raw_uuid(&r.get::<String, _>("id"))),
        name: r.get("name"),
        description: r.get("description"),
        spec: serde_json::from_str(&r.get::<String, _>("spec")).unwrap_or(serde_json::Value::Null),
        created_at: ts(r.get::<i64, _>("created_at")),
    }
}

const COLS: &str = "id, name, description, spec, created_at";

pub async fn list(pool: &MySqlPool, org_id: OrgId) -> DbResult<Vec<MonitorTemplate>> {
    let sql =
        format!("SELECT {COLS} FROM monitor_templates WHERE org_id = ? ORDER BY created_at DESC");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(org_id.0.to_string())
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(template_from).collect())
}

pub async fn get(
    pool: &MySqlPool,
    id: MonitorTemplateId,
    org_id: OrgId,
) -> DbResult<MonitorTemplate> {
    let sql = format!("SELECT {COLS} FROM monitor_templates WHERE id = ? AND org_id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound)?;
    Ok(template_from(&row))
}

pub async fn create(
    pool: &MySqlPool,
    input: NewMonitorTemplate,
    org_id: OrgId,
) -> DbResult<MonitorTemplate> {
    let id = MonitorTemplateId::new();
    sqlx::query("INSERT INTO monitor_templates (id, name, description, spec, org_id) VALUES (?, ?, ?, ?, ?)")
        .bind(id.0.to_string())
        .bind(input.name)
        .bind(input.description)
        .bind(serde_json::to_string(&input.spec).unwrap_or_else(|_| "null".into()))
        .bind(org_id.0.to_string())
        .execute(pool)
        .await?;
    get(pool, id, org_id).await
}

pub async fn delete(pool: &MySqlPool, id: MonitorTemplateId, org_id: OrgId) -> DbResult<()> {
    let r = sqlx::query("DELETE FROM monitor_templates WHERE id = ? AND org_id = ?")
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
        let t = create(
            &pool,
            NewMonitorTemplate {
                name: "api template".into(),
                description: Some("standard http monitor".into()),
                spec: serde_json::json!({ "kind": "http", "url": "https://x" }),
            },
            org,
        )
        .await
        .unwrap();
        assert_eq!(t.name, "api template");
        assert_eq!(t.spec["kind"], "http");
        assert_eq!(list(&pool, org).await.unwrap().len(), 1);
        assert_eq!(
            get(&pool, t.id, org).await.unwrap().description.as_deref(),
            Some("standard http monitor")
        );

        let other = super::super::orgs::create(&pool, "other", "Other")
            .await
            .unwrap();
        assert!(matches!(
            get(&pool, t.id, other.id).await,
            Err(DbError::NotFound)
        ));

        delete(&pool, t.id, org).await.unwrap();
        assert!(matches!(
            delete(&pool, t.id, org).await,
            Err(DbError::NotFound)
        ));
    }
}
