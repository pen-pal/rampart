//! MySQL `templates` domain — notification subject/body templates. Ported from
//! PG. The `notification_templates` table already exists
//! (migrations-mysql/0006_notifications.sql); per-org name uniqueness lands in
//! 0033. `channel_kinds` TEXT[]→LONGTEXT (serde JSON array), `is_default`→
//! TINYINT, no `RETURNING` → re-select; `update` is get-cur-then-set-all.

use super::{raw_uuid, ts};
use crate::templates::{NewTemplate, RenderedTemplate, Template, UpdateTemplate};
use crate::{DbError, DbResult};
use rampart_core::ids::{NotificationTemplateId, OrgId};
use sqlx::{MySqlPool, Row};
use uuid::Uuid;

const COLS: &str = "id, name, channel_kinds, event_kind, subject_template, body_template, \
                    is_default, created_at";

fn template_from(r: &sqlx::mysql::MySqlRow) -> Template {
    Template {
        id: NotificationTemplateId::from_uuid(raw_uuid(&r.get::<String, _>("id"))),
        name: r.get("name"),
        channel_kinds: serde_json::from_str(&r.get::<String, _>("channel_kinds"))
            .unwrap_or_default(),
        event_kind: r.get("event_kind"),
        subject_template: r.get("subject_template"),
        body_template: r.get("body_template"),
        is_default: r.get::<i64, _>("is_default") != 0,
        created_at: ts(r.get::<i64, _>("created_at")),
    }
}

fn unique_conflict(e: sqlx::Error) -> DbError {
    match &e {
        sqlx::Error::Database(d) if d.is_unique_violation() => {
            DbError::Conflict("template name already exists".into())
        }
        _ => DbError::Sqlx(e),
    }
}

pub async fn list(pool: &MySqlPool, org_id: OrgId) -> DbResult<Vec<Template>> {
    let sql = format!(
        "SELECT {COLS} FROM notification_templates WHERE org_id = ? ORDER BY created_at DESC"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(org_id.0.to_string())
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(template_from).collect())
}

pub async fn get(
    pool: &MySqlPool,
    id: NotificationTemplateId,
    org_id: OrgId,
) -> DbResult<Template> {
    let sql = format!("SELECT {COLS} FROM notification_templates WHERE id = ? AND org_id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound)?;
    Ok(template_from(&row))
}

pub async fn create(pool: &MySqlPool, input: NewTemplate, org_id: OrgId) -> DbResult<Template> {
    let id = NotificationTemplateId::from_uuid(Uuid::now_v7());
    sqlx::query(
        "INSERT INTO notification_templates
            (id, name, channel_kinds, event_kind, subject_template, body_template, is_default, org_id)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id.0.to_string())
    .bind(input.name)
    .bind(serde_json::to_string(&input.channel_kinds).unwrap_or_else(|_| "[]".into()))
    .bind(input.event_kind)
    .bind(input.subject_template)
    .bind(input.body_template)
    .bind(input.is_default as i64)
    .bind(org_id.0.to_string())
    .execute(pool)
    .await
    .map_err(unique_conflict)?;
    get(pool, id, org_id).await
}

pub async fn update(
    pool: &MySqlPool,
    id: NotificationTemplateId,
    input: UpdateTemplate,
    org_id: OrgId,
) -> DbResult<Template> {
    let cur = get(pool, id, org_id).await?; // gate + defaults source
    sqlx::query(
        "UPDATE notification_templates
            SET name = ?, channel_kinds = ?, event_kind = ?,
                subject_template = ?, body_template = ?, is_default = ?
          WHERE id = ? AND org_id = ?",
    )
    .bind(input.name.unwrap_or(cur.name))
    .bind(
        serde_json::to_string(&input.channel_kinds.unwrap_or(cur.channel_kinds))
            .unwrap_or_else(|_| "[]".into()),
    )
    .bind(input.event_kind.unwrap_or(cur.event_kind))
    .bind(input.subject_template.unwrap_or(cur.subject_template))
    .bind(input.body_template.unwrap_or(cur.body_template))
    .bind(input.is_default.unwrap_or(cur.is_default) as i64)
    .bind(id.0.to_string())
    .bind(org_id.0.to_string())
    .execute(pool)
    .await
    .map_err(unique_conflict)?;
    get(pool, id, org_id).await
}

pub async fn delete(pool: &MySqlPool, id: NotificationTemplateId, org_id: OrgId) -> DbResult<()> {
    let r = sqlx::query("DELETE FROM notification_templates WHERE id = ? AND org_id = ?")
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .execute(pool)
        .await?;
    if r.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

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

#[cfg(test)]
mod tests {
    use super::*;

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    fn new_t(name: &str) -> NewTemplate {
        serde_json::from_value(serde_json::json!({
            "name": name, "channel_kinds": ["slack", "email"],
            "event_kind": "monitor_down", "body_template": "{{monitor.name}} down",
        }))
        .unwrap()
    }

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn crud(pool: MySqlPool) {
        let org = super::super::oid(DEF);
        let t = create(&pool, new_t("Down alert"), org).await.unwrap();
        assert_eq!(
            t.channel_kinds,
            vec!["slack".to_string(), "email".to_string()]
        );
        assert!(!t.is_default);
        assert_eq!(list(&pool, org).await.unwrap().len(), 1);

        // per-org name conflict.
        assert!(matches!(
            create(&pool, new_t("Down alert"), org).await,
            Err(DbError::Conflict(_))
        ));

        // partial update: clear subject, keep name; flip is_default.
        let u = update(
            &pool,
            t.id,
            serde_json::from_value(
                serde_json::json!({ "is_default": true, "subject_template": null }),
            )
            .unwrap(),
            org,
        )
        .await
        .unwrap();
        assert_eq!(u.name, "Down alert");
        assert!(u.is_default);
        assert!(u.subject_template.is_none());

        // render-strings read.
        assert_eq!(
            get_render_strings(&pool, t.id).await.unwrap().body,
            "{{monitor.name}} down"
        );

        // cross-org isolation + delete.
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
