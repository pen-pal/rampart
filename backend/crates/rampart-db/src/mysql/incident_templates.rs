//! MySQL `incident_templates` domain — canned incident-update bodies. Ported
//! from PG. `incident_style` enum→VARCHAR via serde round-trip; no `RETURNING`
//! → INSERT/UPDATE-then-re-select. `update` is read-modify-write (get-first
//! confirms existence + NotFound, mirroring PG).

use super::{raw_uuid, ts};
use crate::incident_templates::{NewIncidentTemplate, UpdateIncidentTemplate};
use crate::{DbError, DbResult};
use rampart_core::ids::OrgId;
use rampart_core::{IncidentStyle, IncidentTemplate, IncidentTemplateId};
use sqlx::{MySqlPool, Row};

fn style_str(s: IncidentStyle) -> String {
    serde_json::to_value(s)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| "warning".into())
}

fn style_from(s: &str) -> IncidentStyle {
    serde_json::from_value(serde_json::Value::String(s.to_string()))
        .unwrap_or(IncidentStyle::Warning)
}

fn template_from(r: &sqlx::mysql::MySqlRow) -> IncidentTemplate {
    IncidentTemplate {
        id: IncidentTemplateId::from_uuid(raw_uuid(&r.get::<String, _>("id"))),
        name: r.get("name"),
        body: r.get("body"),
        style: style_from(&r.get::<String, _>("style")),
        created_at: ts(r.get::<i64, _>("created_at")),
    }
}

const COLS: &str = "id, name, body, style, created_at";

pub async fn list(pool: &MySqlPool, org_id: OrgId) -> DbResult<Vec<IncidentTemplate>> {
    let sql =
        format!("SELECT {COLS} FROM incident_templates WHERE org_id = ? ORDER BY created_at DESC");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(org_id.0.to_string())
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(template_from).collect())
}

pub async fn get(
    pool: &MySqlPool,
    id: IncidentTemplateId,
    org_id: OrgId,
) -> DbResult<IncidentTemplate> {
    let sql = format!("SELECT {COLS} FROM incident_templates WHERE id = ? AND org_id = ?");
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
    input: NewIncidentTemplate,
    org_id: OrgId,
) -> DbResult<IncidentTemplate> {
    let id = IncidentTemplateId::new();
    sqlx::query(
        "INSERT INTO incident_templates (id, name, body, style, org_id) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(id.0.to_string())
    .bind(input.name)
    .bind(input.body)
    .bind(style_str(input.style))
    .bind(org_id.0.to_string())
    .execute(pool)
    .await?;
    get(pool, id, org_id).await
}

pub async fn update(
    pool: &MySqlPool,
    id: IncidentTemplateId,
    input: UpdateIncidentTemplate,
    org_id: OrgId,
) -> DbResult<IncidentTemplate> {
    let cur = get(pool, id, org_id).await?; // confirms existence + NotFound
    sqlx::query(
        "UPDATE incident_templates SET name = ?, body = ?, style = ? WHERE id = ? AND org_id = ?",
    )
    .bind(input.name.unwrap_or(cur.name))
    .bind(input.body.unwrap_or(cur.body))
    .bind(style_str(input.style.unwrap_or(cur.style)))
    .bind(id.0.to_string())
    .bind(org_id.0.to_string())
    .execute(pool)
    .await?;
    get(pool, id, org_id).await
}

pub async fn delete(pool: &MySqlPool, id: IncidentTemplateId, org_id: OrgId) -> DbResult<()> {
    let r = sqlx::query("DELETE FROM incident_templates WHERE id = ? AND org_id = ?")
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
            serde_json::from_value(serde_json::json!({
                "name": "Investigating", "body": "We are looking into it."
            }))
            .unwrap(),
            org,
        )
        .await
        .unwrap();
        assert_eq!(t.name, "Investigating");
        assert_eq!(t.style, IncidentStyle::Warning); // default
        assert_eq!(list(&pool, org).await.unwrap().len(), 1);

        // update body + style; name kept.
        let u = update(
            &pool,
            t.id,
            serde_json::from_value(serde_json::json!({ "body": "Identified.", "style": "info" }))
                .unwrap(),
            org,
        )
        .await
        .unwrap();
        assert_eq!(u.name, "Investigating");
        assert_eq!(u.body, "Identified.");
        assert_eq!(u.style, IncidentStyle::Info);

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
