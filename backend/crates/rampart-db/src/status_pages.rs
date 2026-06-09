//! Status-page repository.
//!
//! The status_pages table from migration 0001 carries a lot of room
//! for growth (theme, custom_css, custom_domain, password_hash, …).
//! v1 only uses a handful of columns; the rest are NULL for now.

use crate::{heartbeats, DbError, DbPool, DbResult};
use rampart_core::status_page::{
    MonthlyUptimePoint, NewStatusPage, PublicIncident, PublicIncidentUpdate,
    PublicResolvedIncident, PublicStatusMonitor, PublicStatusPage, StatusPage, UpdateStatusPage,
};
use rampart_core::{MonitorId, StatusPageId};
use time::OffsetDateTime;
use uuid::Uuid;

struct PageRow {
    id: Uuid,
    slug: String,
    title: String,
    description: Option<String>,
    theme: String,
    custom_domain: Option<String>,
    logo_url: Option<String>,
    created_at: OffsetDateTime,
}

impl From<PageRow> for StatusPage {
    fn from(r: PageRow) -> Self {
        StatusPage {
            id: StatusPageId::from_uuid(r.id),
            slug: r.slug,
            title: r.title,
            description: r.description,
            theme: r.theme,
            custom_domain: r.custom_domain,
            logo_url: r.logo_url,
            // The existing schema has no `updated_at`. The API response
            // re-uses `created_at` until a future migration adds one.
            created_at: r.created_at,
            updated_at: r.created_at,
            monitor_ids: Vec::new(),
        }
    }
}

pub async fn list(pool: &DbPool) -> DbResult<Vec<StatusPage>> {
    let rows = sqlx::query_as!(
        PageRow,
        r#"
        SELECT id, slug, title, description, theme, custom_domain, logo_url, created_at
        FROM status_pages
        ORDER BY created_at DESC
        "#,
    )
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let edges = sqlx::query!(
        r#"
        SELECT page_id, monitor_id
        FROM status_page_monitors
        WHERE page_id = ANY($1)
        ORDER BY position
        "#,
        &ids,
    )
    .fetch_all(pool)
    .await?;

    let mut pages: Vec<StatusPage> = rows.into_iter().map(Into::into).collect();
    for e in edges {
        if let Some(p) = pages.iter_mut().find(|p| p.id.0 == e.page_id) {
            p.monitor_ids.push(MonitorId::from_uuid(e.monitor_id));
        }
    }
    Ok(pages)
}

pub async fn get(pool: &DbPool, id: StatusPageId) -> DbResult<StatusPage> {
    let row = sqlx::query_as!(
        PageRow,
        r#"
        SELECT id, slug, title, description, theme, custom_domain, logo_url, created_at
        FROM status_pages
        WHERE id = $1
        "#,
        id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;

    hydrate(pool, row).await
}

pub async fn get_by_slug(pool: &DbPool, slug: &str) -> DbResult<StatusPage> {
    let row = sqlx::query_as!(
        PageRow,
        r#"
        SELECT id, slug, title, description, theme, custom_domain, logo_url, created_at
        FROM status_pages
        WHERE slug = $1
        "#,
        slug,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;

    hydrate(pool, row).await
}

async fn hydrate(pool: &DbPool, row: PageRow) -> DbResult<StatusPage> {
    let id = row.id;
    let mut page: StatusPage = row.into();
    let edges = sqlx::query!(
        r#"
        SELECT monitor_id FROM status_page_monitors
        WHERE page_id = $1
        ORDER BY position
        "#,
        id,
    )
    .fetch_all(pool)
    .await?;
    page.monitor_ids = edges
        .into_iter()
        .map(|e| MonitorId::from_uuid(e.monitor_id))
        .collect();
    Ok(page)
}

pub async fn create(pool: &DbPool, input: NewStatusPage) -> DbResult<StatusPage> {
    let id = StatusPageId::new();
    let mut tx = pool.begin().await?;

    sqlx::query!(
        r#"
        INSERT INTO status_pages (id, slug, title, description, theme, custom_domain, logo_url)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
        id.0,
        input.slug,
        input.title,
        input.description,
        input.theme,
        input.custom_domain,
        input.logo_url,
    )
    .execute(&mut *tx)
    .await
    .map_err(map_slug_conflicts)?;

    for (i, mid) in input.monitor_ids.iter().enumerate() {
        sqlx::query!(
            r#"
            INSERT INTO status_page_monitors (page_id, monitor_id, position)
            VALUES ($1, $2, $3)
            ON CONFLICT DO NOTHING
            "#,
            id.0,
            mid.0,
            i as i32,
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    get(pool, id).await
}

pub async fn update(
    pool: &DbPool,
    id: StatusPageId,
    patch: UpdateStatusPage,
) -> DbResult<StatusPage> {
    let mut tx = pool.begin().await?;

    if let Some(title) = patch.title.as_deref() {
        sqlx::query!(
            "UPDATE status_pages SET title = $1 WHERE id = $2",
            title,
            id.0,
        )
        .execute(&mut *tx)
        .await?;
    }
    if let Some(theme) = patch.theme.as_deref() {
        sqlx::query!(
            "UPDATE status_pages SET theme = $1 WHERE id = $2",
            theme,
            id.0,
        )
        .execute(&mut *tx)
        .await?;
    }
    if let Some(desc) = patch.description.as_ref() {
        sqlx::query!(
            "UPDATE status_pages SET description = $1 WHERE id = $2",
            desc.as_deref(),
            id.0,
        )
        .execute(&mut *tx)
        .await?;
    }
    if let Some(domain) = patch.custom_domain.as_ref() {
        sqlx::query!(
            "UPDATE status_pages SET custom_domain = $1 WHERE id = $2",
            domain.as_deref(),
            id.0,
        )
        .execute(&mut *tx)
        .await
        .map_err(map_slug_conflicts)?;
    }
    if let Some(logo) = patch.logo_url.as_ref() {
        sqlx::query!(
            "UPDATE status_pages SET logo_url = $1 WHERE id = $2",
            logo.as_deref(),
            id.0,
        )
        .execute(&mut *tx)
        .await?;
    }

    if let Some(new_ids) = patch.monitor_ids.as_ref() {
        sqlx::query!("DELETE FROM status_page_monitors WHERE page_id = $1", id.0,)
            .execute(&mut *tx)
            .await?;
        for (i, mid) in new_ids.iter().enumerate() {
            sqlx::query!(
                r#"
                INSERT INTO status_page_monitors (page_id, monitor_id, position)
                VALUES ($1, $2, $3)
                ON CONFLICT DO NOTHING
                "#,
                id.0,
                mid.0,
                i as i32,
            )
            .execute(&mut *tx)
            .await?;
        }
    }

    tx.commit().await?;
    get(pool, id).await
}

pub async fn delete(pool: &DbPool, id: StatusPageId) -> DbResult<()> {
    let result = sqlx::query!("DELETE FROM status_pages WHERE id = $1", id.0)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

/// Build the public projection of a page identified by its slug. One
/// extra round trip per attached monitor to roll up 90-day uptime;
/// good enough for status pages with tens of monitors. CTE-based
/// aggregation is the next move if any page grows past that.
pub async fn public_view(pool: &DbPool, slug: &str) -> DbResult<PublicStatusPage> {
    let page = get_by_slug(pool, slug).await?;

    let mut monitors = Vec::with_capacity(page.monitor_ids.len());
    for mid in &page.monitor_ids {
        let m = crate::monitors::get(pool, *mid).await?;
        let uptime = heartbeats::uptime_pct(pool, *mid, 90 * 86400)
            .await?
            .map(|v| v as f32);
        let avg_lat = heartbeats::avg_latency_ms(pool, *mid, 86_400)
            .await?
            .map(|v| v as f32);
        let daily = heartbeats::daily_status(pool, *mid, 90).await?;
        // Vec<u8> of ASCII chars → String. Each byte is one of
        // 'u'/'d'/'w'/'m'/'n' as documented on PublicStatusMonitor.
        let daily_str = String::from_utf8(daily).unwrap_or_default();
        let monthly = heartbeats::monthly_uptime(pool, *mid, 12).await?;
        let monthly_points: Vec<MonthlyUptimePoint> = monthly
            .into_iter()
            .map(|p| MonthlyUptimePoint {
                year_month: p.year_month,
                uptime_pct: p.uptime_pct,
            })
            .collect();
        monitors.push(PublicStatusMonitor {
            name: m.name,
            current_status: m.current_status,
            uptime_90d: uptime,
            avg_latency_ms_24h: avg_lat,
            daily_status_90d: daily_str,
            monthly_uptime_12mo: monthly_points,
        });
    }

    // Pull active incidents + their running updates. Two queries per
    // page (incidents, then updates) — fine for status pages which
    // typically have at most a handful active at once.
    let active = crate::incidents::list_active(pool, page.id).await?;
    let mut incidents = Vec::with_capacity(active.len());
    for inc in active {
        let updates = crate::incidents::list_updates(pool, inc.id).await?;
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

    // Resolved-incident history for the public timeline pane.
    // Cap at 30 — enough to cover a busy month, bounded so a long-
    // lived page can't dump its full history on every scrape.
    let history_rows = crate::incidents::list_resolved_history(pool, page.id, 30).await?;
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

    Ok(PublicStatusPage {
        slug: page.slug,
        title: page.title,
        description: page.description,
        theme: page.theme,
        custom_domain: page.custom_domain,
        logo_url: page.logo_url,
        generated_at: OffsetDateTime::now_utc(),
        monitors,
        incidents,
        incident_history,
    })
}

/// Convert unique / CHECK Postgres errors into a friendlier
/// `DbError::Conflict`. Used by both create (slug + custom_domain) and the
/// custom-domain update path. We disambiguate the duplicate-key case by the
/// violated constraint name so a domain clash doesn't surface as "slug in
/// use". Anything else falls through to the generic sqlx error (→ 500).
fn map_slug_conflicts(e: sqlx::Error) -> DbError {
    match &e {
        sqlx::Error::Database(db) => {
            // 23505 = unique_violation, 23514 = check_violation.
            match db.code().as_deref() {
                Some("23505") => {
                    // Disambiguate by the violated constraint. The custom-domain
                    // uniqueness index is `status_pages_custom_domain_uidx`
                    // (this migration); tolerate the `_key` spelling too in case
                    // an environment already carries a same-purpose constraint.
                    match db.constraint() {
                        Some("status_pages_custom_domain_uidx")
                        | Some("status_pages_custom_domain_key") => {
                            DbError::Conflict("custom domain is already in use".into())
                        }
                        _ => DbError::Conflict("slug is already in use".into()),
                    }
                }
                Some("23514") => DbError::Conflict("slug must match ^[a-z0-9-]{2,40}$".into()),
                _ => DbError::from(e),
            }
        }
        _ => DbError::from(e),
    }
}
