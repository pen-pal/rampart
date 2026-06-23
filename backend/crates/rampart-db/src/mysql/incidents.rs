//! MySQL `incidents` domain — status-page announcements. Ported from PG.
//!
//! Dialect: uuid→CHAR(36), ts→BIGINT, bool→TINYINT(1), `incident_style`
//! enum→VARCHAR(16) (serde round-trip). No `RETURNING` → INSERT-then-re-select.
//! `COALESCE($n, col)` partial-update ports verbatim. `resolve` can't use the
//! `rows_affected()==0 → NotFound` gate (MySQL counts CHANGED rows, so resolving
//! an already-resolved incident is a 0-change no-op) — it disambiguates
//! missing-vs-no-op with an existence probe so re-resolve stays idempotent.

use super::{raw_uuid, ts, uid};
use crate::incidents::{NewIncident, UpdateIncident};
use crate::{DbError, DbResult};
use rampart_core::ids::OrgId;
use rampart_core::{
    Incident, IncidentId, IncidentStyle, IncidentUpdate, IncidentUpdateId, StatusPageId, UserId,
};
use sqlx::{MySqlPool, Row};
use time::OffsetDateTime;
use uuid::Uuid;

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

const COLS: &str =
    "id, status_page_id, title, content, style, pinned, active, resolved_at, created_at, created_by, dedup_key";

fn incident_from(r: &sqlx::mysql::MySqlRow) -> Incident {
    Incident {
        id: IncidentId::from_uuid(raw_uuid(&r.get::<String, _>("id"))),
        status_page_id: StatusPageId::from_uuid(raw_uuid(&r.get::<String, _>("status_page_id"))),
        title: r.get("title"),
        content: r.get("content"),
        style: style_from(&r.get::<String, _>("style")),
        pinned: r.get::<i64, _>("pinned") != 0,
        active: r.get::<i64, _>("active") != 0,
        resolved_at: r.get::<Option<i64>, _>("resolved_at").map(ts),
        created_at: ts(r.get::<i64, _>("created_at")),
        created_by: r.get::<Option<String>, _>("created_by").map(|s| uid(&s)),
        dedup_key: r.get("dedup_key"),
    }
}

pub async fn create(
    pool: &MySqlPool,
    page: StatusPageId,
    author: Option<UserId>,
    input: NewIncident,
) -> DbResult<Incident> {
    let id = IncidentId::new();
    sqlx::query(
        "INSERT INTO incidents
            (id, status_page_id, title, content, style, pinned, created_by, dedup_key)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id.0.to_string())
    .bind(page.0.to_string())
    .bind(input.title)
    .bind(input.content)
    .bind(style_str(input.style))
    .bind(input.pinned as i64)
    .bind(author.map(|u| u.0.to_string()))
    .bind(input.dedup_key)
    .execute(pool)
    .await?;
    get(pool, id).await
}

pub async fn find_active_by_dedup_key(
    pool: &MySqlPool,
    page: StatusPageId,
    key: &str,
) -> DbResult<Option<Incident>> {
    let sql = format!(
        "SELECT {COLS} FROM incidents
         WHERE status_page_id = ? AND active = 1 AND dedup_key = ?
         ORDER BY created_at DESC LIMIT 1"
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(page.0.to_string())
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(row.as_ref().map(incident_from))
}

pub async fn list_active(pool: &MySqlPool, page: StatusPageId) -> DbResult<Vec<Incident>> {
    let sql = format!(
        "SELECT {COLS} FROM incidents
         WHERE status_page_id = ? AND active = 1
         ORDER BY pinned DESC, created_at DESC"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(page.0.to_string())
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(incident_from).collect())
}

pub async fn recent(pool: &MySqlPool, limit: i64, org_id: OrgId) -> DbResult<Vec<Incident>> {
    let sql = format!(
        "SELECT {}
         FROM incidents i
         JOIN status_pages sp ON sp.id = i.status_page_id
         WHERE sp.org_id = ?
         ORDER BY i.active DESC, i.created_at DESC
         LIMIT ?",
        // qualify each column with the incidents alias.
        COLS.split(", ")
            .map(|c| format!("i.{c}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(org_id.0.to_string())
        .bind(limit.clamp(1, 50))
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(incident_from).collect())
}

pub async fn list_resolved_history(
    pool: &MySqlPool,
    page: StatusPageId,
    limit: i64,
) -> DbResult<Vec<Incident>> {
    let sql = format!(
        "SELECT {COLS} FROM incidents
         WHERE status_page_id = ? AND active = 0
         ORDER BY COALESCE(resolved_at, created_at) DESC
         LIMIT ?"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(page.0.to_string())
        .bind(limit)
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(incident_from).collect())
}

pub async fn resolve(pool: &MySqlPool, id: IncidentId, now: OffsetDateTime) -> DbResult<()> {
    let res = sqlx::query(
        "UPDATE incidents SET active = 0, resolved_at = COALESCE(resolved_at, ?) WHERE id = ?",
    )
    .bind(now.unix_timestamp())
    .bind(id.0.to_string())
    .execute(pool)
    .await?;
    // 0 changed rows = either missing, or already-resolved no-op. Probe to keep
    // re-resolve idempotent (Ok) while a genuinely-absent id still 404s.
    if res.rows_affected() == 0 {
        let exists = sqlx::query("SELECT 1 FROM incidents WHERE id = ?")
            .bind(id.0.to_string())
            .fetch_optional(pool)
            .await?;
        if exists.is_none() {
            return Err(DbError::NotFound);
        }
    }
    Ok(())
}

pub async fn list_all(pool: &MySqlPool, page: StatusPageId, limit: i64) -> DbResult<Vec<Incident>> {
    let sql = format!(
        "SELECT {COLS} FROM incidents
         WHERE status_page_id = ?
         ORDER BY active DESC, pinned DESC, created_at DESC
         LIMIT ?"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(page.0.to_string())
        .bind(limit.clamp(1, 500))
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(incident_from).collect())
}

pub async fn delete(pool: &MySqlPool, id: IncidentId) -> DbResult<()> {
    let res = sqlx::query("DELETE FROM incidents WHERE id = ?")
        .bind(id.0.to_string())
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

pub async fn update(pool: &MySqlPool, id: IncidentId, patch: UpdateIncident) -> DbResult<Incident> {
    // COALESCE keeps unset fields. No rows_affected gate (MySQL counts CHANGED
    // rows → a no-op patch would falsely 404); the trailing get() surfaces a
    // genuinely-missing id as NotFound.
    sqlx::query(
        "UPDATE incidents SET
            title   = COALESCE(?, title),
            content = COALESCE(?, content),
            style   = COALESCE(?, style),
            pinned  = COALESCE(?, pinned)
         WHERE id = ?",
    )
    .bind(patch.title)
    .bind(patch.content)
    .bind(patch.style.map(style_str))
    .bind(patch.pinned.map(|b| b as i64))
    .bind(id.0.to_string())
    .execute(pool)
    .await?;
    get(pool, id).await
}

pub async fn get(pool: &MySqlPool, id: IncidentId) -> DbResult<Incident> {
    let sql = format!("SELECT {COLS} FROM incidents WHERE id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.0.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound)?;
    Ok(incident_from(&row))
}

pub async fn list_updates(pool: &MySqlPool, incident: IncidentId) -> DbResult<Vec<IncidentUpdate>> {
    let rows = sqlx::query(
        "SELECT id, incident_id, message, posted_at, posted_by
         FROM incident_updates WHERE incident_id = ? ORDER BY posted_at ASC",
    )
    .bind(incident.0.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| IncidentUpdate {
            id: IncidentUpdateId::from_uuid(raw_uuid(&r.get::<String, _>("id"))),
            incident_id: IncidentId::from_uuid(raw_uuid(&r.get::<String, _>("incident_id"))),
            message: r.get("message"),
            posted_at: ts(r.get::<i64, _>("posted_at")),
            posted_by: r.get::<Option<String>, _>("posted_by").map(|s| uid(&s)),
        })
        .collect())
}

pub async fn post_update(
    pool: &MySqlPool,
    incident: IncidentId,
    author: Option<UserId>,
    message: String,
) -> DbResult<Uuid> {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO incident_updates (id, incident_id, message, posted_by) VALUES (?, ?, ?, ?)",
    )
    .bind(id.to_string())
    .bind(incident.0.to_string())
    .bind(message)
    .bind(author.map(|u| u.0.to_string()))
    .execute(pool)
    .await?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Seed a status page row directly (status_pages domain ships next slice).
    async fn seed_page(pool: &MySqlPool, org: OrgId, slug: &str) -> StatusPageId {
        let id = StatusPageId::new();
        sqlx::query("INSERT INTO status_pages (id, slug, title, org_id) VALUES (?, ?, ?, ?)")
            .bind(id.0.to_string())
            .bind(slug)
            .bind("Page")
            .bind(org.0.to_string())
            .execute(pool)
            .await
            .unwrap();
        id
    }

    fn new_inc(title: &str, dedup: Option<&str>) -> NewIncident {
        NewIncident {
            title: title.into(),
            content: "investigating".into(),
            style: IncidentStyle::Danger,
            pinned: true,
            dedup_key: dedup.map(String::from),
        }
    }

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn crud_dedup_resolve_updates(pool: MySqlPool) {
        let org = super::super::oid("00000000-0000-0000-0000-000000000001");
        let page = seed_page(&pool, org, "main").await;

        let inc = create(&pool, page, None, new_inc("DB down", Some("k1")))
            .await
            .unwrap();
        assert_eq!(inc.style, IncidentStyle::Danger);
        assert!(inc.active && inc.pinned);

        // dedup lookup hits the active row; a different key misses.
        assert_eq!(
            find_active_by_dedup_key(&pool, page, "k1")
                .await
                .unwrap()
                .unwrap()
                .id,
            inc.id
        );
        assert!(find_active_by_dedup_key(&pool, page, "nope")
            .await
            .unwrap()
            .is_none());

        // running update + listing.
        post_update(&pool, inc.id, None, "still looking".into())
            .await
            .unwrap();
        assert_eq!(list_updates(&pool, inc.id).await.unwrap().len(), 1);

        // partial edit keeps title, flips style.
        let edited = update(
            &pool,
            inc.id,
            UpdateIncident {
                title: None,
                content: Some("found it".into()),
                style: Some(IncidentStyle::Info),
                pinned: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(edited.title, "DB down");
        assert_eq!(edited.content, "found it");
        assert_eq!(edited.style, IncidentStyle::Info);

        assert_eq!(list_active(&pool, page).await.unwrap().len(), 1);
        assert_eq!(recent(&pool, 10, org).await.unwrap().len(), 1);

        // resolve, then re-resolve is idempotent (not NotFound).
        resolve(&pool, inc.id, OffsetDateTime::now_utc())
            .await
            .unwrap();
        resolve(&pool, inc.id, OffsetDateTime::now_utc())
            .await
            .unwrap();
        assert!(list_active(&pool, page).await.unwrap().is_empty());
        assert_eq!(
            list_resolved_history(&pool, page, 10).await.unwrap().len(),
            1
        );
        assert!(get(&pool, inc.id).await.unwrap().resolved_at.is_some());

        // resolve a ghost id → NotFound.
        assert!(matches!(
            resolve(&pool, IncidentId::new(), OffsetDateTime::now_utc()).await,
            Err(DbError::NotFound)
        ));

        // cross-org recent isolation.
        let other = super::super::orgs::create(&pool, "other", "Other")
            .await
            .unwrap();
        assert!(recent(&pool, 10, other.id).await.unwrap().is_empty());

        assert_eq!(list_all(&pool, page, 10).await.unwrap().len(), 1);
        delete(&pool, inc.id).await.unwrap();
        assert!(matches!(
            delete(&pool, inc.id).await,
            Err(DbError::NotFound)
        ));
    }
}
