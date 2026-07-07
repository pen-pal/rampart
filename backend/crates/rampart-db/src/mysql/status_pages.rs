//! MySQL `status_pages` domain — branded public status pages + their component
//! sections + the public projection. Ported from PG.
//!
//! Dialect: uuid→CHAR(36), ts→BIGINT, bool→TINYINT, `password_hash IS NOT NULL`
//! → the derived `private` boolean (the Argon2 hash never leaves the db on a
//! read path). No `RETURNING` → INSERT-then-re-select. `= ANY($1)` → bound
//! `IN (?,…)`. `ON CONFLICT DO NOTHING` → `INSERT IGNORE`. **No FK cascade**
//! (the MySQL tier keeps FKs documentary) → `delete` cascades child rows
//! (monitors, sections, incidents+updates) in a tx by hand. Duplicate-key →
//! `Conflict`, disambiguated slug-vs-custom_domain by the error message.
//!
//! `public_view` rolls up every attached monitor's 90-day uptime / latency /
//! daily strip / monthly chips with set-based batch queries (five total,
//! regardless of monitor count), mirroring the PG amplification fix.

use super::{in_placeholders, raw_uuid, ts};
use crate::{DbError, DbResult};
use argon2::password_hash::{rand_core::OsRng, SaltString};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use rampart_core::ids::OrgId;
use rampart_core::status_page::{
    MonitorAssignment, MonthlyUptimePoint, NewStatusPage, NewStatusPageSection, PublicIncident,
    PublicIncidentUpdate, PublicResolvedIncident, PublicSection, PublicStatusMonitor,
    PublicStatusPage, StatusPage, StatusPageSection, UpdateStatusPage, UpdateStatusPageSection,
};
use rampart_core::{MonitorId, StatusPageId, StatusPageSectionId};
use sqlx::{MySqlPool, Row};
use std::collections::HashMap;
use time::OffsetDateTime;
use uuid::Uuid;

fn hash_page_password(plaintext: &str) -> DbResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(plaintext.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| DbError::Conflict(format!("could not hash password: {e}")))
}

/// MySQL duplicate-key → friendly `Conflict`, disambiguating slug vs
/// custom_domain by the violated key named in the error message.
fn map_slug_conflicts(e: sqlx::Error) -> DbError {
    if let sqlx::Error::Database(db) = &e {
        let msg = db.message();
        if msg.contains("Duplicate entry") {
            if msg.contains("custom_domain") {
                return DbError::Conflict("custom domain is already in use".into());
            }
            return DbError::Conflict("slug is already in use".into());
        }
    }
    DbError::from(e)
}

const PAGE_COLS: &str =
    "id, slug, title, description, theme, custom_domain, logo_url, custom_css, \
                         (password_hash IS NOT NULL) AS private, created_at";

fn page_from(r: &sqlx::mysql::MySqlRow) -> StatusPage {
    let created = ts(r.get::<i64, _>("created_at"));
    StatusPage {
        id: StatusPageId::from_uuid(raw_uuid(&r.get::<String, _>("id"))),
        slug: r.get("slug"),
        title: r.get("title"),
        description: r.get("description"),
        theme: r.get("theme"),
        custom_domain: r.get("custom_domain"),
        logo_url: r.get("logo_url"),
        custom_css: r.get("custom_css"),
        private: r.get::<i64, _>("private") != 0,
        // No `updated_at` column; the API reuses `created_at` (matches PG).
        created_at: created,
        updated_at: created,
        monitor_ids: Vec::new(),
        monitor_sections: Vec::new(),
    }
}

pub async fn list(pool: &MySqlPool, org_id: OrgId) -> DbResult<Vec<StatusPage>> {
    list_inner(pool, Some(org_id)).await
}

pub async fn list_all(pool: &MySqlPool) -> DbResult<Vec<StatusPage>> {
    list_inner(pool, None).await
}

async fn list_inner(pool: &MySqlPool, org_id: Option<OrgId>) -> DbResult<Vec<StatusPage>> {
    let org_filter = org_id.map(|o| o.0.to_string());
    let sql = format!(
        "SELECT {PAGE_COLS} FROM status_pages WHERE (? IS NULL OR org_id = ?) ORDER BY created_at DESC"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(&org_filter)
        .bind(&org_filter)
        .fetch_all(pool)
        .await?;
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let mut pages: Vec<StatusPage> = rows.iter().map(page_from).collect();

    let edge_sql = format!(
        "SELECT page_id, monitor_id FROM status_page_monitors WHERE page_id IN ({}) ORDER BY position",
        in_placeholders(pages.len())
    );
    let mut q = sqlx::query(sqlx::AssertSqlSafe(edge_sql));
    for p in pages.iter() {
        q = q.bind(p.id.0.to_string());
    }
    let edges = q.fetch_all(pool).await?;
    for e in &edges {
        let page_id = raw_uuid(&e.get::<String, _>("page_id"));
        if let Some(p) = pages.iter_mut().find(|p| p.id.0 == page_id) {
            p.monitor_ids.push(MonitorId::from_uuid(raw_uuid(
                &e.get::<String, _>("monitor_id"),
            )));
        }
    }
    Ok(pages)
}

async fn fetch_one(
    pool: &MySqlPool,
    where_clause: &str,
    bind: &str,
) -> DbResult<Option<StatusPage>> {
    let sql = format!("SELECT {PAGE_COLS} FROM status_pages WHERE {where_clause}");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(bind)
        .fetch_optional(pool)
        .await?;
    match row {
        Some(r) => Ok(Some(hydrate(pool, page_from(&r)).await?)),
        None => Ok(None),
    }
}

pub async fn get(pool: &MySqlPool, id: StatusPageId, org_id: OrgId) -> DbResult<StatusPage> {
    let sql = format!("SELECT {PAGE_COLS} FROM status_pages WHERE id = ? AND org_id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound)?;
    hydrate(pool, page_from(&row)).await
}

pub async fn get_by_slug(pool: &MySqlPool, slug: &str) -> DbResult<StatusPage> {
    fetch_one(pool, "slug = ?", slug)
        .await?
        .ok_or(DbError::NotFound)
}

pub async fn get_unscoped(pool: &MySqlPool, id: StatusPageId) -> DbResult<StatusPage> {
    fetch_one(pool, "id = ?", &id.0.to_string())
        .await?
        .ok_or(DbError::NotFound)
}

pub async fn find_by_custom_domain(pool: &MySqlPool, host: &str) -> DbResult<Option<StatusPage>> {
    fetch_one(pool, "custom_domain = ?", host).await
}

async fn hydrate(pool: &MySqlPool, mut page: StatusPage) -> DbResult<StatusPage> {
    let edges = sqlx::query(
        "SELECT monitor_id, section_id FROM status_page_monitors WHERE page_id = ? ORDER BY position",
    )
    .bind(page.id.0.to_string())
    .fetch_all(pool)
    .await?;
    page.monitor_ids = edges
        .iter()
        .map(|e| MonitorId::from_uuid(raw_uuid(&e.get::<String, _>("monitor_id"))))
        .collect();
    page.monitor_sections = edges
        .iter()
        .map(|e| MonitorAssignment {
            monitor_id: MonitorId::from_uuid(raw_uuid(&e.get::<String, _>("monitor_id"))),
            section_id: e
                .get::<Option<String>, _>("section_id")
                .map(|s| StatusPageSectionId::from_uuid(raw_uuid(&s))),
        })
        .collect();
    Ok(page)
}

pub async fn create(pool: &MySqlPool, input: NewStatusPage, org_id: OrgId) -> DbResult<StatusPage> {
    let id = StatusPageId::new();
    let password_hash = match input.password.as_deref() {
        Some(pw) if !pw.is_empty() => Some(hash_page_password(pw)?),
        _ => None,
    };
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO status_pages
            (id, slug, title, description, theme, custom_domain, logo_url, custom_css, password_hash, org_id)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id.0.to_string())
    .bind(input.slug)
    .bind(input.title)
    .bind(input.description)
    .bind(input.theme)
    .bind(input.custom_domain)
    .bind(input.logo_url)
    .bind(input.custom_css)
    .bind(password_hash)
    .bind(org_id.0.to_string())
    .execute(&mut *tx)
    .await
    .map_err(map_slug_conflicts)?;

    for (i, mid) in input.monitor_ids.iter().enumerate() {
        sqlx::query(
            "INSERT IGNORE INTO status_page_monitors (page_id, monitor_id, position) VALUES (?, ?, ?)",
        )
        .bind(id.0.to_string())
        .bind(mid.0.to_string())
        .bind(i as i32)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    get_unscoped(pool, id).await
}

pub async fn update(
    pool: &MySqlPool,
    id: StatusPageId,
    patch: UpdateStatusPage,
    org_id: OrgId,
) -> DbResult<StatusPage> {
    // Org-ownership gate scopes the whole tx (a cross-org id 404s before any
    // mutation; every UPDATE below targets this id).
    get(pool, id, org_id).await?;
    let mut tx = pool.begin().await?;
    let sid = id.0.to_string();

    if let Some(title) = patch.title.as_deref() {
        sqlx::query("UPDATE status_pages SET title = ? WHERE id = ?")
            .bind(title)
            .bind(&sid)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(theme) = patch.theme.as_deref() {
        sqlx::query("UPDATE status_pages SET theme = ? WHERE id = ?")
            .bind(theme)
            .bind(&sid)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(desc) = patch.description.as_ref() {
        sqlx::query("UPDATE status_pages SET description = ? WHERE id = ?")
            .bind(desc.as_deref())
            .bind(&sid)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(domain) = patch.custom_domain.as_ref() {
        sqlx::query("UPDATE status_pages SET custom_domain = ? WHERE id = ?")
            .bind(domain.as_deref())
            .bind(&sid)
            .execute(&mut *tx)
            .await
            .map_err(map_slug_conflicts)?;
    }
    if let Some(logo) = patch.logo_url.as_ref() {
        sqlx::query("UPDATE status_pages SET logo_url = ? WHERE id = ?")
            .bind(logo.as_deref())
            .bind(&sid)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(css) = patch.custom_css.as_ref() {
        sqlx::query("UPDATE status_pages SET custom_css = ? WHERE id = ?")
            .bind(css.as_deref())
            .bind(&sid)
            .execute(&mut *tx)
            .await?;
    }
    // Password: Some(Some(pw)) sets (→private), Some(None)/empty clears (→public),
    // None leaves untouched.
    if let Some(pw_opt) = patch.password.as_ref() {
        let new_hash = match pw_opt.as_deref() {
            Some(pw) if !pw.is_empty() => Some(hash_page_password(pw)?),
            _ => None,
        };
        sqlx::query("UPDATE status_pages SET password_hash = ? WHERE id = ?")
            .bind(new_hash)
            .bind(&sid)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(new_ids) = patch.monitor_ids.as_ref() {
        sqlx::query("DELETE FROM status_page_monitors WHERE page_id = ?")
            .bind(&sid)
            .execute(&mut *tx)
            .await?;
        for (i, mid) in new_ids.iter().enumerate() {
            sqlx::query(
                "INSERT IGNORE INTO status_page_monitors (page_id, monitor_id, position) VALUES (?, ?, ?)",
            )
            .bind(&sid)
            .bind(mid.0.to_string())
            .bind(i as i32)
            .execute(&mut *tx)
            .await?;
        }
    }
    tx.commit().await?;
    get(pool, id, org_id).await
}

pub async fn delete(pool: &MySqlPool, id: StatusPageId, org_id: OrgId) -> DbResult<()> {
    let sid = id.0.to_string();
    let mut tx = pool.begin().await?;
    // No FK cascade on the MySQL tier → remove child rows by hand, inner-most
    // first (incident_updates → incidents → page edges → sections → page).
    sqlx::query(
        "DELETE FROM incident_updates WHERE incident_id IN
            (SELECT id FROM incidents WHERE status_page_id = ?)",
    )
    .bind(&sid)
    .execute(&mut *tx)
    .await?;
    for tbl in [
        "DELETE FROM incidents WHERE status_page_id = ?",
        "DELETE FROM status_page_monitors WHERE page_id = ?",
        "DELETE FROM status_page_sections WHERE status_page_id = ?",
    ] {
        sqlx::query(tbl).bind(&sid).execute(&mut *tx).await?;
    }
    let res = sqlx::query("DELETE FROM status_pages WHERE id = ? AND org_id = ?")
        .bind(&sid)
        .bind(org_id.0.to_string())
        .execute(&mut *tx)
        .await?;
    if res.rows_affected() == 0 {
        // Roll back the child deletes — the page wasn't ours (or is gone).
        tx.rollback().await?;
        return Err(DbError::NotFound);
    }
    tx.commit().await?;
    Ok(())
}

pub async fn public_view(pool: &MySqlPool, slug: &str) -> DbResult<PublicStatusPage> {
    let page = get_by_slug(pool, slug).await?;

    let ids: Vec<Uuid> = page.monitor_ids.iter().map(|m| m.0).collect();
    let fields = crate::mysql::monitors::public_fields_batch(pool, &ids).await?;
    let uptime_map = crate::mysql::heartbeats::uptime_pct_batch(pool, &ids, 90 * 86400).await?;
    let avg_lat_map = crate::mysql::heartbeats::avg_latency_ms_batch(pool, &ids, 86_400).await?;
    let daily_map = crate::mysql::heartbeats::daily_status_batch(pool, &ids, 90).await?;
    let monthly_map = crate::mysql::heartbeats::monthly_uptime_batch(pool, &ids, 12).await?;

    let mut monitors = Vec::with_capacity(page.monitor_ids.len());
    let mut by_id: Vec<(MonitorId, PublicStatusMonitor)> =
        Vec::with_capacity(page.monitor_ids.len());
    for mid in &page.monitor_ids {
        let (name, current_status) = fields.get(&mid.0).cloned().ok_or(DbError::NotFound)?;
        let uptime = uptime_map.get(&mid.0).map(|v| *v as f32);
        let avg_lat = avg_lat_map.get(&mid.0).map(|v| *v as f32);
        let daily = daily_map.get(&mid.0).cloned().unwrap_or_default();
        let daily_str = String::from_utf8(daily).unwrap_or_default();
        let monthly = monthly_map.get(&mid.0).cloned().unwrap_or_default();
        let monthly_points: Vec<MonthlyUptimePoint> = monthly
            .into_iter()
            .map(|p| MonthlyUptimePoint {
                year_month: p.year_month,
                uptime_pct: p.uptime_pct,
            })
            .collect();
        let pm = PublicStatusMonitor {
            name,
            current_status,
            uptime_90d: uptime,
            avg_latency_ms_24h: avg_lat,
            daily_status_90d: daily_str,
            monthly_uptime_12mo: monthly_points,
        };
        monitors.push(pm.clone());
        by_id.push((*mid, pm));
    }

    // Partition into sections (synthetic ungrouped bucket first, then labelled
    // sections in position order). Identical bucketing to PG.
    let section_rows = list_sections(pool, page.id).await?;
    let assignments =
        sqlx::query("SELECT monitor_id, section_id FROM status_page_monitors WHERE page_id = ?")
            .bind(page.id.0.to_string())
            .fetch_all(pool)
            .await?;
    let mut section_of: HashMap<Uuid, Option<Uuid>> = HashMap::with_capacity(assignments.len());
    for a in &assignments {
        section_of.insert(
            raw_uuid(&a.get::<String, _>("monitor_id")),
            a.get::<Option<String>, _>("section_id")
                .map(|s| raw_uuid(&s)),
        );
    }

    let mut ungrouped: Vec<PublicStatusMonitor> = Vec::new();
    let mut grouped: Vec<(Uuid, String, Vec<PublicStatusMonitor>)> = section_rows
        .iter()
        .map(|s| (s.id.0, s.name.clone(), Vec::new()))
        .collect();
    for (mid, pm) in by_id {
        match section_of.get(&mid.0).copied().flatten() {
            Some(sid) => match grouped.iter_mut().find(|(id, _, _)| *id == sid) {
                Some((_, _, ms)) => ms.push(pm),
                None => ungrouped.push(pm),
            },
            None => ungrouped.push(pm),
        }
    }

    let mut sections: Vec<PublicSection> = Vec::with_capacity(grouped.len() + 1);
    if !ungrouped.is_empty() {
        sections.push(PublicSection {
            name: None,
            monitors: ungrouped,
        });
    }
    for (_, name, ms) in grouped {
        sections.push(PublicSection {
            name: Some(name),
            monitors: ms,
        });
    }

    // Active incidents + their running updates.
    let active = crate::mysql::incidents::list_active(pool, page.id).await?;
    let mut incidents = Vec::with_capacity(active.len());
    for inc in active {
        let updates = crate::mysql::incidents::list_updates(pool, inc.id).await?;
        incidents.push(PublicIncident {
            title: inc.title,
            content: inc.content,
            style: inc.style,
            pinned: inc.pinned,
            created_at: inc.created_at,
            updates: updates
                .into_iter()
                .map(|u| PublicIncidentUpdate {
                    message: u.message,
                    posted_at: u.posted_at,
                })
                .collect(),
        });
    }

    let history_rows = crate::mysql::incidents::list_resolved_history(pool, page.id, 30).await?;
    let incident_history = history_rows
        .into_iter()
        .filter_map(|inc| {
            inc.resolved_at.map(|resolved_at| PublicResolvedIncident {
                title: inc.title,
                content: inc.content,
                style: inc.style,
                created_at: inc.created_at,
                resolved_at,
            })
        })
        .collect();

    let maintenance = crate::mysql::maintenance::public_for_status_page(pool, page.id).await?;

    Ok(PublicStatusPage {
        slug: page.slug,
        title: page.title,
        description: page.description,
        theme: page.theme,
        custom_domain: page.custom_domain,
        logo_url: page.logo_url,
        custom_css: page.custom_css,
        private: page.private,
        generated_at: OffsetDateTime::now_utc(),
        monitors,
        sections,
        incidents,
        incident_history,
        maintenance,
    })
}

pub async fn verify_page_password(pool: &MySqlPool, slug: &str, candidate: &str) -> DbResult<bool> {
    let row = sqlx::query("SELECT password_hash FROM status_pages WHERE slug = ?")
        .bind(slug)
        .fetch_optional(pool)
        .await?;
    let Some(hash) = row.and_then(|r| r.get::<Option<String>, _>("password_hash")) else {
        return Ok(false);
    };
    Ok(PasswordHash::new(&hash)
        .and_then(|h| Argon2::default().verify_password(candidate.as_bytes(), &h))
        .is_ok())
}

// ── Component sections ──────────────────────────────────────────────────

fn section_from(r: &sqlx::mysql::MySqlRow) -> StatusPageSection {
    StatusPageSection {
        id: StatusPageSectionId::from_uuid(raw_uuid(&r.get::<String, _>("id"))),
        status_page_id: StatusPageId::from_uuid(raw_uuid(&r.get::<String, _>("status_page_id"))),
        name: r.get("name"),
        position: r.get::<i32, _>("position"),
    }
}

pub async fn list_sections(
    pool: &MySqlPool,
    page_id: StatusPageId,
) -> DbResult<Vec<StatusPageSection>> {
    let rows = sqlx::query(
        "SELECT id, status_page_id, name, position FROM status_page_sections
         WHERE status_page_id = ? ORDER BY position, name",
    )
    .bind(page_id.0.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(section_from).collect())
}

pub async fn create_section(
    pool: &MySqlPool,
    page_id: StatusPageId,
    input: NewStatusPageSection,
) -> DbResult<StatusPageSection> {
    let id = StatusPageSectionId::new();
    // Append after the current last section.
    let (pos,): (i64,) = sqlx::query_as(
        "SELECT COALESCE(MAX(position) + 1, 0) FROM status_page_sections WHERE status_page_id = ?",
    )
    .bind(page_id.0.to_string())
    .fetch_one(pool)
    .await?;
    sqlx::query(
        "INSERT INTO status_page_sections (id, status_page_id, name, position) VALUES (?, ?, ?, ?)",
    )
    .bind(id.0.to_string())
    .bind(page_id.0.to_string())
    .bind(input.name)
    .bind(pos as i32)
    .execute(pool)
    .await?;
    get_section(pool, id).await
}

async fn get_section(pool: &MySqlPool, id: StatusPageSectionId) -> DbResult<StatusPageSection> {
    let row = sqlx::query(
        "SELECT id, status_page_id, name, position FROM status_page_sections WHERE id = ?",
    )
    .bind(id.0.to_string())
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(section_from(&row))
}

/// A section belongs to the given (org-gated) page, else NotFound. Guards the
/// by-id section mutations against cross-org tampering.
async fn section_in_page(
    pool: &MySqlPool,
    page_id: StatusPageId,
    id: StatusPageSectionId,
) -> DbResult<()> {
    let hit: Option<String> = sqlx::query_scalar(
        "SELECT id FROM status_page_sections WHERE id = ? AND status_page_id = ?",
    )
    .bind(id.0.to_string())
    .bind(page_id.0.to_string())
    .fetch_optional(pool)
    .await?;
    if hit.is_none() {
        return Err(DbError::NotFound);
    }
    Ok(())
}

pub async fn update_section(
    pool: &MySqlPool,
    page_id: StatusPageId,
    id: StatusPageSectionId,
    patch: UpdateStatusPageSection,
) -> DbResult<StatusPageSection> {
    section_in_page(pool, page_id, id).await?;
    if let Some(name) = patch.name.as_deref() {
        sqlx::query("UPDATE status_page_sections SET name = ? WHERE id = ?")
            .bind(name)
            .bind(id.0.to_string())
            .execute(pool)
            .await?;
    }
    if let Some(pos) = patch.position {
        sqlx::query("UPDATE status_page_sections SET position = ? WHERE id = ?")
            .bind(pos)
            .bind(id.0.to_string())
            .execute(pool)
            .await?;
    }
    get_section(pool, id).await
}

pub async fn delete_section(
    pool: &MySqlPool,
    page_id: StatusPageId,
    id: StatusPageSectionId,
) -> DbResult<()> {
    section_in_page(pool, page_id, id).await?;
    // FK is ON DELETE SET NULL in PG; the MySQL tier has no FK, so detach the
    // section's monitors back to ungrouped first, then drop the section.
    sqlx::query("UPDATE status_page_monitors SET section_id = NULL WHERE section_id = ?")
        .bind(id.0.to_string())
        .execute(pool)
        .await?;
    let res = sqlx::query("DELETE FROM status_page_sections WHERE id = ?")
        .bind(id.0.to_string())
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

pub async fn assign_monitor_section(
    pool: &MySqlPool,
    page_id: StatusPageId,
    monitor_id: MonitorId,
    section_id: Option<StatusPageSectionId>,
) -> DbResult<()> {
    let res = sqlx::query(
        "UPDATE status_page_monitors SET section_id = ? WHERE page_id = ? AND monitor_id = ?",
    )
    .bind(section_id.map(|s| s.0.to_string()))
    .bind(page_id.0.to_string())
    .bind(monitor_id.0.to_string())
    .execute(pool)
    .await?;
    // rows_affected==0 here means the monitor isn't attached to the page (a real
    // miss), not a no-op: setting to the same value still matches 0 changed rows
    // in MySQL, so probe to avoid a false NotFound.
    if res.rows_affected() == 0 {
        let (exists,): (i64,) = sqlx::query_as(
            "SELECT EXISTS(SELECT 1 FROM status_page_monitors WHERE page_id = ? AND monitor_id = ?)",
        )
        .bind(page_id.0.to_string())
        .bind(monitor_id.0.to_string())
        .fetch_one(pool)
        .await?;
        if exists == 0 {
            return Err(DbError::NotFound);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    fn new_page(slug: &str) -> NewStatusPage {
        serde_json::from_value(serde_json::json!({
            "slug": slug, "title": "Acme", "theme": "dark",
        }))
        .unwrap()
    }

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn crud_sections_password_publicview(pool: MySqlPool) {
        let org = super::super::oid(DEF);

        let p = create(&pool, new_page("acme"), org).await.unwrap();
        assert_eq!(p.slug, "acme");
        assert_eq!(p.theme, "dark");
        assert!(!p.private);
        assert_eq!(list(&pool, org).await.unwrap().len(), 1);
        assert_eq!(get_by_slug(&pool, "acme").await.unwrap().id, p.id);

        // duplicate slug → Conflict (friendly).
        assert!(matches!(
            create(&pool, new_page("acme"), org).await,
            Err(DbError::Conflict(_))
        ));

        // set a password (→ private) + verify.
        update(
            &pool,
            p.id,
            serde_json::from_value(serde_json::json!({ "password": "hunter2" })).unwrap(),
            org,
        )
        .await
        .unwrap();
        assert!(get(&pool, p.id, org).await.unwrap().private);
        assert!(verify_page_password(&pool, "acme", "hunter2")
            .await
            .unwrap());
        assert!(!verify_page_password(&pool, "acme", "wrong").await.unwrap());
        // clear it (→ public again).
        update(
            &pool,
            p.id,
            serde_json::from_value(serde_json::json!({ "password": null })).unwrap(),
            org,
        )
        .await
        .unwrap();
        assert!(!get(&pool, p.id, org).await.unwrap().private);

        // sections: append two, rename one, then delete.
        let s1 = create_section(
            &pool,
            p.id,
            serde_json::from_value(serde_json::json!({ "name": "API" })).unwrap(),
        )
        .await
        .unwrap();
        let s2 = create_section(
            &pool,
            p.id,
            serde_json::from_value(serde_json::json!({ "name": "Web" })).unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(s2.position, 1);
        assert_eq!(list_sections(&pool, p.id).await.unwrap().len(), 2);
        let r = update_section(
            &pool,
            s1.id,
            serde_json::from_value(serde_json::json!({ "name": "Backend" })).unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(r.name, "Backend");

        // attach a monitor, assign it to s2, public_view buckets it.
        let m = super::super::monitors::create(
            &pool,
            serde_json::from_value(serde_json::json!({
                "name": "api", "kind": "http", "url": "https://x",
                "interval_seconds": 60, "timeout_seconds": 10, "max_retries": 0,
                "retry_interval_sec": 60, "resend_interval_sec": 0, "upside_down": false,
                "http_method": "GET", "accepted_statuses": [200],
                "follow_redirect": true, "ignore_tls": false, "check_cert": false,
                "cert_expiry_days": 14
            }))
            .unwrap(),
            org,
        )
        .await
        .unwrap()
        .id;
        update(
            &pool,
            p.id,
            serde_json::from_value(serde_json::json!({ "monitor_ids": [m.0.to_string()] }))
                .unwrap(),
            org,
        )
        .await
        .unwrap();
        assign_monitor_section(&pool, p.id, m, Some(s2.id))
            .await
            .unwrap();

        let pv = public_view(&pool, "acme").await.unwrap();
        assert_eq!(pv.monitors.len(), 1);
        assert_eq!(pv.monitors[0].daily_status_90d.len(), 90);
        // one labelled section ("Web") holds the monitor; "Backend" is empty;
        // no ungrouped bucket.
        assert!(pv
            .sections
            .iter()
            .any(|s| s.name.as_deref() == Some("Web") && s.monitors.len() == 1));
        assert!(!pv.sections.iter().any(|s| s.name.is_none()));

        // deleting a section detaches its monitor back to ungrouped.
        delete_section(&pool, s2.id).await.unwrap();
        let pv2 = public_view(&pool, "acme").await.unwrap();
        assert!(pv2
            .sections
            .iter()
            .any(|s| s.name.is_none() && s.monitors.len() == 1));

        // cross-org get + delete cascade.
        let other = super::super::orgs::create(&pool, "other", "Other")
            .await
            .unwrap();
        assert!(matches!(
            get(&pool, p.id, other.id).await,
            Err(DbError::NotFound)
        ));
        assert!(matches!(
            delete(&pool, p.id, other.id).await,
            Err(DbError::NotFound)
        ));
        delete(&pool, p.id, org).await.unwrap();
        assert!(list(&pool, org).await.unwrap().is_empty());
        assert!(list_sections(&pool, p.id).await.unwrap().is_empty());
    }
}
