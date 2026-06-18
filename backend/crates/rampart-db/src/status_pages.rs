//! Status-page repository.
//!
//! The status_pages table carries branding (custom_domain, logo_url),
//! a per-page custom_css override, and an optional Argon2 password_hash
//! that flips the page private. The hash never leaves the db on a read
//! path — projections expose only a derived `private` boolean.

use crate::{heartbeats, DbError, DbPool, DbResult};
use argon2::password_hash::{rand_core::OsRng, SaltString};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use rampart_core::status_page::{
    MonitorAssignment, MonthlyUptimePoint, NewStatusPage, NewStatusPageSection, PublicIncident,
    PublicIncidentUpdate, PublicResolvedIncident, PublicSection, PublicStatusMonitor,
    PublicStatusPage, StatusPage, StatusPageSection, UpdateStatusPage, UpdateStatusPageSection,
};
use rampart_core::ids::OrgId;
use rampart_core::{MonitorId, StatusPageId, StatusPageSectionId};
use time::OffsetDateTime;
use uuid::Uuid;

/// Hash a plaintext status-page password with Argon2id + a random salt,
/// returning the PHC string we store in `status_pages.password_hash`.
/// Mirrors the user-auth hashing in `rampart-api::auth`; kept here so the
/// db layer owns the plaintext→hash transition the API hands it.
fn hash_page_password(plaintext: &str) -> DbResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(plaintext.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| DbError::Conflict(format!("could not hash password: {e}")))
}

struct PageRow {
    id: Uuid,
    slug: String,
    title: String,
    description: Option<String>,
    theme: String,
    custom_domain: Option<String>,
    logo_url: Option<String>,
    custom_css: Option<String>,
    /// `password_hash IS NOT NULL` — the row is private. We deliberately
    /// SELECT the derived boolean rather than the hash so the secret never
    /// leaves the database into application memory on read paths.
    private: bool,
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
            custom_css: r.custom_css,
            private: r.private,
            // The existing schema has no `updated_at`. The API response
            // re-uses `created_at` until a future migration adds one.
            created_at: r.created_at,
            updated_at: r.created_at,
            monitor_ids: Vec::new(),
            monitor_sections: Vec::new(),
        }
    }
}

/// Status pages owned by one org — the management list (org-scoped).
pub async fn list(pool: &DbPool, org_id: OrgId) -> DbResult<Vec<StatusPage>> {
    list_inner(pool, Some(org_id)).await
}

/// Every status page across ALL orgs — for system callers (seed/import). Not
/// org-scoped; never call on a request path (use [`list`]).
pub async fn list_all(pool: &DbPool) -> DbResult<Vec<StatusPage>> {
    list_inner(pool, None).await
}

async fn list_inner(pool: &DbPool, org_id: Option<OrgId>) -> DbResult<Vec<StatusPage>> {
    let org_filter = org_id.map(|o| o.0);
    let rows = sqlx::query_as!(
        PageRow,
        r#"
        SELECT id, slug, title, description, theme, custom_domain, logo_url, custom_css,
               (password_hash IS NOT NULL) AS "private!", created_at
        FROM status_pages
        WHERE ($1::uuid IS NULL OR org_id = $1)
        ORDER BY created_at DESC
        "#,
        org_filter,
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

/// Fetch one page scoped to an org (cross-org id → NotFound). Management path;
/// the public view resolves by slug/host via [`get_by_slug`] /
/// [`find_by_custom_domain`], which stay unscoped.
pub async fn get(pool: &DbPool, id: StatusPageId, org_id: OrgId) -> DbResult<StatusPage> {
    let row = sqlx::query_as!(
        PageRow,
        r#"
        SELECT id, slug, title, description, theme, custom_domain, logo_url, custom_css,
               (password_hash IS NOT NULL) AS "private!", created_at
        FROM status_pages
        WHERE id = $1 AND org_id = $2
        "#,
        id.0,
        org_id.0,
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
        SELECT id, slug, title, description, theme, custom_domain, logo_url, custom_css,
               (password_hash IS NOT NULL) AS "private!", created_at
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

/// Fetch one page WITHOUT org scoping — for the just-inserted-row return in
/// [`create`]. Not for request paths (use [`get`]).
pub async fn get_unscoped(pool: &DbPool, id: StatusPageId) -> DbResult<StatusPage> {
    let row = sqlx::query_as!(
        PageRow,
        r#"
        SELECT id, slug, title, description, theme, custom_domain, logo_url, custom_css,
               (password_hash IS NOT NULL) AS "private!", created_at
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

/// Resolve a status page by its configured `custom_domain` (the Host
/// header value). Returns `None` when no page claims the host — callers
/// (host-header routing) fall through to the normal dashboard flow rather
/// than treating a miss as an error. The partial UNIQUE index on
/// `custom_domain` guarantees at most one row matches.
pub async fn find_by_custom_domain(pool: &DbPool, host: &str) -> DbResult<Option<StatusPage>> {
    let row = sqlx::query_as!(
        PageRow,
        r#"
        SELECT id, slug, title, description, theme, custom_domain, logo_url, custom_css,
               (password_hash IS NOT NULL) AS "private!", created_at
        FROM status_pages
        WHERE custom_domain = $1
        "#,
        host,
    )
    .fetch_optional(pool)
    .await?;

    match row {
        Some(row) => Ok(Some(hydrate(pool, row).await?)),
        None => Ok(None),
    }
}

async fn hydrate(pool: &DbPool, row: PageRow) -> DbResult<StatusPage> {
    let id = row.id;
    let mut page: StatusPage = row.into();
    let edges = sqlx::query!(
        r#"
        SELECT monitor_id, section_id FROM status_page_monitors
        WHERE page_id = $1
        ORDER BY position
        "#,
        id,
    )
    .fetch_all(pool)
    .await?;
    page.monitor_ids = edges
        .iter()
        .map(|e| MonitorId::from_uuid(e.monitor_id))
        .collect();
    page.monitor_sections = edges
        .into_iter()
        .map(|e| MonitorAssignment {
            monitor_id: MonitorId::from_uuid(e.monitor_id),
            section_id: e.section_id.map(StatusPageSectionId::from_uuid),
        })
        .collect();
    Ok(page)
}

pub async fn create(
    pool: &DbPool,
    input: NewStatusPage,
    org_id: rampart_core::ids::OrgId,
) -> DbResult<StatusPage> {
    let id = StatusPageId::new();
    // Hash the plaintext password (if any) before opening the tx so a hash
    // failure aborts cleanly without a dangling transaction.
    let password_hash = match input.password.as_deref() {
        Some(pw) if !pw.is_empty() => Some(hash_page_password(pw)?),
        _ => None,
    };
    let mut tx = pool.begin().await?;

    sqlx::query!(
        r#"
        INSERT INTO status_pages
            (id, slug, title, description, theme, custom_domain, logo_url, custom_css, password_hash, org_id)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        "#,
        id.0,
        input.slug,
        input.title,
        input.description,
        input.theme,
        input.custom_domain,
        input.logo_url,
        input.custom_css,
        password_hash,
        org_id.0,
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
    get_unscoped(pool, id).await
}

pub async fn update(
    pool: &DbPool,
    id: StatusPageId,
    patch: UpdateStatusPage,
    org_id: OrgId,
) -> DbResult<StatusPage> {
    // Org-ownership gate: a page in another org 404s before any mutation. The
    // per-field UPDATEs below all target this id, so the single check scopes
    // the whole transaction.
    get(pool, id, org_id).await?;
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
    if let Some(css) = patch.custom_css.as_ref() {
        sqlx::query!(
            "UPDATE status_pages SET custom_css = $1 WHERE id = $2",
            css.as_deref(),
            id.0,
        )
        .execute(&mut *tx)
        .await?;
    }
    // Password: `Some(Some(pw))` sets (hash → private), `Some(None)` clears
    // (→ public), `None` leaves it untouched. An empty string is treated as
    // a clear so the builder can blank the field to remove protection.
    if let Some(pw_opt) = patch.password.as_ref() {
        let new_hash = match pw_opt.as_deref() {
            Some(pw) if !pw.is_empty() => Some(hash_page_password(pw)?),
            _ => None,
        };
        sqlx::query!(
            "UPDATE status_pages SET password_hash = $1 WHERE id = $2",
            new_hash,
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
    get(pool, id, org_id).await
}

pub async fn delete(pool: &DbPool, id: StatusPageId, org_id: OrgId) -> DbResult<()> {
    let result = sqlx::query!(
        "DELETE FROM status_pages WHERE id = $1 AND org_id = $2",
        id.0,
        org_id.0,
    )
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

    // Build the flat, page-ordered monitor projection (back-compat) once.
    // We keep each monitor's id alongside its projection so we can partition
    // the same projections into sections below without re-querying.
    let mut monitors = Vec::with_capacity(page.monitor_ids.len());
    let mut by_id: Vec<(MonitorId, PublicStatusMonitor)> =
        Vec::with_capacity(page.monitor_ids.len());
    for mid in &page.monitor_ids {
        // Monitors linked to a (public) status page; the page is the org-scoped
        // root, resolved in the status-pages domain phase. Fetch unscoped here.
        let m = crate::monitors::get_unscoped(pool, *mid).await?;
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
        let pm = PublicStatusMonitor {
            name: m.name,
            current_status: m.current_status,
            uptime_90d: uptime,
            avg_latency_ms_24h: avg_lat,
            daily_status_90d: daily_str,
            monthly_uptime_12mo: monthly_points,
        };
        monitors.push(pm.clone());
        by_id.push((*mid, pm));
    }

    // Partition the projections into public sections. Read the page's
    // sections (ordered) plus the monitor→section assignments, then bucket:
    // a synthetic leading ungrouped entry (name = None) for monitors with no
    // section, followed by one entry per labelled section in `position`
    // order. The ungrouped entry is only emitted when it actually has
    // monitors, so a fully-grouped page renders no headerless bucket.
    let section_rows = list_sections(pool, page.id).await?;
    let assignments = sqlx::query!(
        r#"
        SELECT monitor_id, section_id
        FROM status_page_monitors
        WHERE page_id = $1
        "#,
        page.id.0,
    )
    .fetch_all(pool)
    .await?;
    let mut section_of: std::collections::HashMap<Uuid, Option<Uuid>> =
        std::collections::HashMap::with_capacity(assignments.len());
    for a in assignments {
        section_of.insert(a.monitor_id, a.section_id);
    }

    let mut ungrouped: Vec<PublicStatusMonitor> = Vec::new();
    // Preserve section order; each holds its monitors in page order.
    let mut grouped: Vec<(Uuid, String, Vec<PublicStatusMonitor>)> = section_rows
        .iter()
        .map(|s| (s.id.0, s.name.clone(), Vec::new()))
        .collect();
    for (mid, pm) in by_id {
        match section_of.get(&mid.0).copied().flatten() {
            Some(sid) => match grouped.iter_mut().find(|(id, _, _)| *id == sid) {
                Some((_, _, ms)) => ms.push(pm),
                // Dangling section_id (shouldn't happen given the FK) → treat
                // as ungrouped rather than dropping the monitor.
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

    // Active + upcoming (<=7d) maintenance whose windows touch any monitor
    // on this page. Drives the public banner above the components list.
    let maintenance = crate::maintenance::public_for_status_page(pool, page.id).await?;

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

/// Verify a candidate password against a page's stored Argon2 hash.
/// Returns `false` for a missing page, a public page (no hash), or a
/// mismatch — the caller treats all three the same ("not unlocked"). Never
/// short-circuits on length, so it doesn't leak timing about the hash.
pub async fn verify_page_password(pool: &DbPool, slug: &str, candidate: &str) -> DbResult<bool> {
    let row = sqlx::query!(
        "SELECT password_hash FROM status_pages WHERE slug = $1",
        slug,
    )
    .fetch_optional(pool)
    .await?;

    let Some(hash) = row.and_then(|r| r.password_hash) else {
        return Ok(false);
    };
    Ok(PasswordHash::new(&hash)
        .and_then(|h| Argon2::default().verify_password(candidate.as_bytes(), &h))
        .is_ok())
}

// ── Component sections (sub-section grouping) ──────────────────────────

struct SectionRow {
    id: Uuid,
    status_page_id: Uuid,
    name: String,
    position: i32,
}

impl From<SectionRow> for StatusPageSection {
    fn from(r: SectionRow) -> Self {
        StatusPageSection {
            id: StatusPageSectionId::from_uuid(r.id),
            status_page_id: StatusPageId::from_uuid(r.status_page_id),
            name: r.name,
            position: r.position,
        }
    }
}

/// List a page's sections in display (`position`) order.
pub async fn list_sections(
    pool: &DbPool,
    page_id: StatusPageId,
) -> DbResult<Vec<StatusPageSection>> {
    let rows = sqlx::query_as!(
        SectionRow,
        r#"
        SELECT id, status_page_id, name, position
        FROM status_page_sections
        WHERE status_page_id = $1
        ORDER BY position, name
        "#,
        page_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

/// Create a section on a page, appended after the current last section
/// (position = current count). Returns the freshly created row.
pub async fn create_section(
    pool: &DbPool,
    page_id: StatusPageId,
    input: NewStatusPageSection,
) -> DbResult<StatusPageSection> {
    let id = StatusPageSectionId::new();
    // New sections land at the end. `COALESCE(MAX(position)+1, 0)` keeps the
    // append ordering stable without the caller having to compute it.
    let pos = sqlx::query_scalar!(
        r#"
        SELECT COALESCE(MAX(position) + 1, 0) AS "pos!"
        FROM status_page_sections
        WHERE status_page_id = $1
        "#,
        page_id.0,
    )
    .fetch_one(pool)
    .await?;

    let row = sqlx::query_as!(
        SectionRow,
        r#"
        INSERT INTO status_page_sections (id, status_page_id, name, position)
        VALUES ($1, $2, $3, $4)
        RETURNING id, status_page_id, name, position
        "#,
        id.0,
        page_id.0,
        input.name,
        pos,
    )
    .fetch_one(pool)
    .await?;
    Ok(row.into())
}

/// Rename and/or reposition a section. Fields left `None` are untouched.
pub async fn update_section(
    pool: &DbPool,
    id: StatusPageSectionId,
    patch: UpdateStatusPageSection,
) -> DbResult<StatusPageSection> {
    if let Some(name) = patch.name.as_deref() {
        sqlx::query!(
            "UPDATE status_page_sections SET name = $1 WHERE id = $2",
            name,
            id.0,
        )
        .execute(pool)
        .await?;
    }
    if let Some(pos) = patch.position {
        sqlx::query!(
            "UPDATE status_page_sections SET position = $1 WHERE id = $2",
            pos,
            id.0,
        )
        .execute(pool)
        .await?;
    }
    let row = sqlx::query_as!(
        SectionRow,
        r#"
        SELECT id, status_page_id, name, position
        FROM status_page_sections
        WHERE id = $1
        "#,
        id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(row.into())
}

/// Delete a section. The `status_page_monitors.section_id` FK is
/// `ON DELETE SET NULL`, so its monitors fall back to ungrouped rather than
/// detaching from the page.
pub async fn delete_section(pool: &DbPool, id: StatusPageSectionId) -> DbResult<()> {
    let result = sqlx::query!("DELETE FROM status_page_sections WHERE id = $1", id.0)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

/// Assign a monitor (already attached to the page) to a section, or
/// un-assign it when `section_id` is `None`. Errors `NotFound` when the
/// monitor isn't attached to the page so a stray id doesn't silently no-op.
pub async fn assign_monitor_section(
    pool: &DbPool,
    page_id: StatusPageId,
    monitor_id: MonitorId,
    section_id: Option<StatusPageSectionId>,
) -> DbResult<()> {
    let result = sqlx::query!(
        r#"
        UPDATE status_page_monitors
        SET section_id = $1
        WHERE page_id = $2 AND monitor_id = $3
        "#,
        section_id.map(|s| s.0),
        page_id.0,
        monitor_id.0,
    )
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
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
