//! `/v1/status-pages` (admin CRUD) and `/v1/public/status-pages/:slug`
//! (unauthenticated public view).
//!
//! The admin half lives behind the session middleware just like every
//! other /v1 route. The public half intentionally does NOT — the page
//! is meant to be linked from external sites.

use crate::auth::OrgContext;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use rampart_core::ids::{IncidentId, MonitorId, StatusPageId, StatusPageSectionId};
use rampart_core::incident::{Incident, IncidentUpdate};
use rampart_core::status_page::{
    AssignSectionReq, NewStatusPage, NewStatusPageSection, PublicStatusPage, StatusPage,
    StatusPageSection, UpdateStatusPage, UpdateStatusPageSection,
};
use rampart_db::users::User;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;
use validator::Validate;

pub fn admin_router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", get(get_one).patch(update).delete(remove))
        // Component sections (sub-section grouping).
        .route("/{id}/sections", get(list_sections).post(create_section))
        .route(
            "/{id}/sections/{section_id}",
            axum::routing::patch(update_section).delete(delete_section),
        )
        // Assign (or clear, via section_id: null) a monitor's section.
        .route(
            "/{id}/monitors/{monitor_id}/section",
            axum::routing::put(assign_monitor_section),
        )
}

pub fn public_router(limiter: crate::rate_limit::IpRateLimiter) -> Router<AppState> {
    // `/unlock` runs a memory-hard Argon2id verify (~19 MiB) on an
    // unauthenticated path, so a known private slug lets an anonymous client
    // flood cheap POSTs and pin CPU/RAM. Throttle it per-IP with the same
    // limiter as `/auth` (10-burst / ~10-per-min). The cheap public read GETs
    // below are polled by every status-page viewer, so they stay unthrottled.
    let unlock = Router::new()
        .route("/{slug}/unlock", axum::routing::post(public_unlock))
        .route_layer(axum::middleware::from_fn_with_state(
            limiter,
            crate::rate_limit::enforce_ip_rate_limit,
        ));
    Router::new()
        .route("/by-domain/{host}", get(public_view_by_domain))
        .route("/{slug}", get(public_view))
        .route("/{slug}/feed.atom", get(public_feed_atom))
        .route("/{slug}/feed.rss", get(public_feed_rss))
        .route(
            "/{slug}/incidents/{incident_id}/feed.atom",
            get(public_incident_feed_atom),
        )
        .route(
            "/{slug}/incidents/{incident_id}/feed.rss",
            get(public_incident_feed_rss),
        )
        .route("/{slug}/day-latency", get(public_day_latency))
        .merge(unlock)
}

fn parse(id: &str) -> Result<StatusPageId, ApiError> {
    Uuid::from_str(id)
        .map(StatusPageId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid status page id".into()))
}

/// Upper bound on a `data:` URI logo, matching the 512 KB client-side cap.
/// The base64 payload inflates the raw bytes ~33%, so we measure the whole
/// URI string and allow a little headroom for the `data:` prefix.
const MAX_LOGO_DATA_URI_BYTES: usize = 512 * 1024;

/// Upper bound on per-page custom CSS. 64 KB is far more than any sane
/// theme override needs while still bounding the row + the inline <style>
/// the public page injects.
const MAX_CUSTOM_CSS_BYTES: usize = 64 * 1024;

/// Validate optional per-page custom CSS — only a size cap. The content is
/// operator-authored and injected into a scoped <style> tag client-side
/// (which strips any close-out), so we don't sanitize here beyond the cap.
fn validate_custom_css(css: Option<&str>) -> Result<(), ApiError> {
    if let Some(c) = css {
        if c.len() > MAX_CUSTOM_CSS_BYTES {
            return Err(ApiError::BadRequest(
                "custom_css exceeds the 64 KB limit".into(),
            ));
        }
    }
    Ok(())
}

/// Validate an optional custom domain. A bare hostname: 1–253 chars of
/// lowercase letters, digits, dots, and hyphens. We don't enforce the
/// finer RFC-1123 label rules (no leading/trailing hyphen per label) —
/// this is a light guard against obviously bad input, not a resolver.
fn validate_custom_domain(domain: Option<&str>) -> Result<(), ApiError> {
    let Some(d) = domain else { return Ok(()) };
    if d.is_empty() || d.len() > 253 {
        return Err(ApiError::BadRequest(
            "custom_domain must be 1-253 characters".into(),
        ));
    }
    if !d
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
    {
        return Err(ApiError::BadRequest(
            "custom_domain may contain only lowercase letters, digits, dots, and hyphens".into(),
        ));
    }
    Ok(())
}

/// Validate an optional logo URL. A `data:` URI is capped at 512 KB
/// (a base64-encoded uploaded image); anything else must parse as an
/// absolute http(s) URL.
fn validate_logo_url(logo: Option<&str>) -> Result<(), ApiError> {
    let Some(u) = logo else { return Ok(()) };
    if u.is_empty() {
        return Ok(());
    }
    if u.starts_with("data:") {
        if u.len() > MAX_LOGO_DATA_URI_BYTES {
            return Err(ApiError::BadRequest(
                "logo data URI exceeds the 512 KB limit".into(),
            ));
        }
        return Ok(());
    }
    // Not a data URI → require a plausible absolute http(s) URL. We keep
    // the parse dependency-free: scheme + non-empty host.
    let rest = u
        .strip_prefix("https://")
        .or_else(|| u.strip_prefix("http://"));
    match rest {
        Some(host_and_path) if !host_and_path.is_empty() && !host_and_path.starts_with('/') => {
            Ok(())
        }
        _ => Err(ApiError::BadRequest(
            "logo_url must be an http(s) URL or a data: URI".into(),
        )),
    }
}

async fn list(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
) -> Result<Json<Vec<StatusPage>>, ApiError> {
    Ok(Json(s.store().list_status_pages(org.org_id).await?))
}

async fn get_one(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Path(id): Path<String>,
) -> Result<Json<StatusPage>, ApiError> {
    Ok(Json(
        s.store().get_status_page(parse(&id)?, org.org_id).await?,
    ))
}

async fn create(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    Extension(org): Extension<OrgContext>,
    headers: HeaderMap,
    Json(input): Json<NewStatusPage>,
) -> Result<(StatusCode, Json<StatusPage>), ApiError> {
    input
        .validate()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    validate_custom_domain(input.custom_domain.as_deref())?;
    validate_logo_url(input.logo_url.as_deref())?;
    validate_custom_css(input.custom_css.as_deref())?;
    let slug = input.slug.clone();
    let p = s.store().create_status_page(input, org.org_id).await?;
    crate::audit::record(
        s.store(),
        &user,
        &headers,
        "status_page.create",
        "status_page",
        Some(p.id.0),
        Some(serde_json::json!({ "slug": slug })),
    )
    .await;
    invalidate_public_view_cache();
    Ok((StatusCode::CREATED, Json(p)))
}

async fn update(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Path(id): Path<String>,
    Json(input): Json<UpdateStatusPage>,
) -> Result<Json<StatusPage>, ApiError> {
    input
        .validate()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    // Only validate the branding fields when the patch actually carries a
    // value to set (Some(Some(_))). A clear (Some(None)) or an omitted
    // field (None) needs no shape check.
    if let Some(Some(domain)) = input.custom_domain.as_ref() {
        validate_custom_domain(Some(domain))?;
    }
    if let Some(Some(logo)) = input.logo_url.as_ref() {
        validate_logo_url(Some(logo))?;
    }
    if let Some(Some(css)) = input.custom_css.as_ref() {
        validate_custom_css(Some(css))?;
    }
    let page = s
        .store()
        .update_status_page(parse(&id)?, input, org.org_id)
        .await?;
    invalidate_public_view_cache();
    Ok(Json(page))
}

async fn remove(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    Extension(org): Extension<OrgContext>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let page_id = parse(&id)?;
    s.store().delete_status_page(page_id, org.org_id).await?;
    crate::audit::record(
        s.store(),
        &user,
        &headers,
        "status_page.delete",
        "status_page",
        Some(page_id.0),
        None,
    )
    .await;
    invalidate_public_view_cache();
    Ok(StatusCode::NO_CONTENT)
}

fn parse_section(id: &str) -> Result<StatusPageSectionId, ApiError> {
    Uuid::from_str(id)
        .map(StatusPageSectionId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid section id".into()))
}

fn parse_monitor(id: &str) -> Result<MonitorId, ApiError> {
    Uuid::from_str(id)
        .map(MonitorId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid monitor id".into()))
}

async fn list_sections(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Path(id): Path<String>,
) -> Result<Json<Vec<StatusPageSection>>, ApiError> {
    let page_id = parse(&id)?;
    // Org-gate via the parent page (sections inherit org from it); a cross-org
    // page id 404s here before any section is touched.
    s.store().get_status_page(page_id, org.org_id).await?;
    Ok(Json(s.store().list_status_page_sections(page_id).await?))
}

async fn create_section(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Path(id): Path<String>,
    Json(input): Json<NewStatusPageSection>,
) -> Result<(StatusCode, Json<StatusPageSection>), ApiError> {
    input
        .validate()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let page_id = parse(&id)?;
    s.store().get_status_page(page_id, org.org_id).await?;
    let section = s.store().create_status_page_section(page_id, input).await?;
    invalidate_public_view_cache();
    Ok((StatusCode::CREATED, Json(section)))
}

async fn update_section(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Path((id, section_id)): Path<(String, String)>,
    Json(input): Json<UpdateStatusPageSection>,
) -> Result<Json<StatusPageSection>, ApiError> {
    input
        .validate()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    // Gate via the parent page from the path; the mutation is constrained to
    // that page too, so a foreign section id can't be tampered with.
    let page_id = parse(&id)?;
    s.store().get_status_page(page_id, org.org_id).await?;
    let section = s
        .store()
        .update_status_page_section(page_id, parse_section(&section_id)?, input)
        .await?;
    invalidate_public_view_cache();
    Ok(Json(section))
}

async fn delete_section(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Path((id, section_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let page_id = parse(&id)?;
    s.store().get_status_page(page_id, org.org_id).await?;
    s.store()
        .delete_status_page_section(page_id, parse_section(&section_id)?)
        .await?;
    invalidate_public_view_cache();
    Ok(StatusCode::NO_CONTENT)
}

async fn assign_monitor_section(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Path((id, monitor_id)): Path<(String, String)>,
    Json(req): Json<AssignSectionReq>,
) -> Result<StatusCode, ApiError> {
    let page_id = parse(&id)?;
    s.store().get_status_page(page_id, org.org_id).await?;
    s.store()
        .assign_status_page_monitor_section(page_id, parse_monitor(&monitor_id)?, req.section_id)
        .await?;
    invalidate_public_view_cache();
    Ok(StatusCode::NO_CONTENT)
}

/// Build the public projection for `slug`, but when the page is private
/// return a locked stub (slug + title + `private: true`, nothing else)
/// instead of the real component / incident data. A visitor unlocks the
/// real payload via `POST .../unlock`. We do one cheap `get_by_slug` to
/// learn the privacy flag before deciding whether to run the full rollup.
// The public projection costs ~5 queries per attached monitor, on an
// unauthenticated path — so a popular page under an incident (many concurrent
// viewers + browser auto-refresh) can hammer the DB. A short per-slug TTL cache
// collapses that burst into one rollup per window; staleness is bounded by the
// TTL, well within a status page's expectations.
const PUBLIC_VIEW_TTL: std::time::Duration = std::time::Duration::from_secs(10);

fn public_view_cache() -> &'static std::sync::Mutex<
    std::collections::HashMap<String, (std::time::Instant, PublicStatusPage)>,
> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, (std::time::Instant, PublicStatusPage)>>,
    > = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// Drop all cached public projections. Called after any write that can change
/// what a public status page shows (incidents, sections, page settings) so the
/// next read re-projects fresh instead of serving the 10s-stale cache.
pub fn invalidate_public_view_cache() {
    if let Ok(mut cache) = public_view_cache().lock() {
        cache.clear();
    }
}

async fn public_view_guarded(s: &AppState, slug: &str) -> Result<PublicStatusPage, ApiError> {
    let page = s.store().get_status_page_by_slug(slug).await?;
    if page.private {
        // Locked stub is cheap + must reflect the live privacy flag — don't cache.
        return Ok(PublicStatusPage::locked(page.slug, page.title));
    }
    // Serve a fresh cached projection if one exists.
    if let Ok(cache) = public_view_cache().lock() {
        if let Some((at, view)) = cache.get(slug) {
            if at.elapsed() < PUBLIC_VIEW_TTL {
                return Ok(view.clone());
            }
        }
    }
    let view = s.store().status_page_public_view(slug).await?;
    if let Ok(mut cache) = public_view_cache().lock() {
        // Opportunistically evict expired entries so the map can't grow
        // unbounded as slugs come and go.
        cache.retain(|_, (at, _)| at.elapsed() < PUBLIC_VIEW_TTL);
        cache.insert(slug.to_string(), (std::time::Instant::now(), view.clone()));
    }
    Ok(view)
}

async fn public_view(
    State(s): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<PublicStatusPage>, ApiError> {
    Ok(Json(public_view_guarded(&s, &slug).await?))
}

/// Body for `POST /v1/public/status-pages/{slug}/unlock`.
#[derive(Debug, Deserialize)]
pub struct UnlockReq {
    pub password: String,
}

/// Verify a candidate password for a private page. On success returns the
/// full `PublicStatusPage` payload (the frontend keeps it in memory for the
/// session); on a wrong password returns 401. A public page (no password)
/// also 401s here — `/unlock` is only meaningful for private pages, and we
/// don't want to leak which non-private slugs exist via a 200. The 401 body
/// is plain (no auth-redirect semantics): this route lives under
/// `/v1/public/*`, which the frontend calls outside the shared wrapper.
async fn public_unlock(
    State(s): State<AppState>,
    Path(slug): Path<String>,
    Json(req): Json<UnlockReq>,
) -> Result<Json<PublicStatusPage>, ApiError> {
    let ok = s
        .store()
        .verify_status_page_password(&slug, &req.password)
        .await?;
    if !ok {
        return Err(ApiError::Unauthorized);
    }
    Ok(Json(s.store().status_page_public_view(&slug).await?))
}

/// Public view resolved by the page's `custom_domain` rather than its slug.
/// Backs the frontend host-header probe: a visitor hitting `status.acme.com`
/// loads this on boot and, if it resolves, renders the public status view in
/// place of the dashboard shell. We resolve the domain to a slug first, then
/// reuse the same `public_view` projection so the payload is byte-identical to
/// the slug route.
///
/// A domain with no matching page returns `200 null` (not 404): this endpoint
/// is fired as a probe on *every* dashboard boot (the bare-hash visitor), and
/// a 404 there shows up as a red console error on the operator's own host even
/// though it's the expected "this host isn't a custom domain" answer. Returning
/// `null` keeps the probe silent; the frontend already treats a falsy payload
/// as "not a custom-domain host" (App.jsx) / "page not found" (StatusPageView).
async fn public_view_by_domain(
    State(s): State<AppState>,
    Path(host): Path<String>,
) -> Result<Json<Option<PublicStatusPage>>, ApiError> {
    let Some(page) = s.store().find_status_page_by_custom_domain(&host).await? else {
        return Err(ApiError::NotFound);
    };
    Ok(Json(Some(public_view_guarded(&s, &page.slug).await?)))
}

/// Query for `GET /v1/public/status-pages/:slug/day-latency`.
///
/// `monitor_idx` is the 0-based position of the monitor on the public
/// page (same ordering as `PublicStatusPage.monitors`). Index, not ID,
/// because the public projection deliberately doesn't leak monitor UUIDs.
///
/// `date` is the UTC calendar day to bucket; serialized as `YYYY-MM-DD`.
#[derive(Debug, Deserialize)]
pub struct DayLatencyQuery {
    pub monitor_idx: usize,
    #[serde(with = "date_iso")]
    pub date: time::Date,
}

mod date_iso {
    //! `time::Date` serde adapter accepting `YYYY-MM-DD` strings — the
    //! shape the frontend mints from `Date.toISOString().slice(0,10)`.
    use serde::{Deserialize, Deserializer};
    use time::format_description::well_known::Iso8601;

    pub fn deserialize<'de, D>(d: D) -> Result<time::Date, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: String = String::deserialize(d)?;
        time::Date::parse(&s, &Iso8601::DATE).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Serialize)]
pub struct DayLatencyHour {
    pub hour: i32,
    pub avg_latency_ms: Option<f32>,
    pub samples: i32,
}

#[derive(Debug, Serialize)]
pub struct DayLatencyDto {
    pub hours: Vec<DayLatencyHour>,
}

/// Per-hour avg latency for the UTC day specified, scoped to the monitor
/// at `monitor_idx` on the public page. Backs the day-drilldown popover
/// mini-chart. Returns a dense 24-entry vector — hours with no successful
/// heartbeats land with `avg_latency_ms: null` so the frontend can
/// render a muted no-data bar at that position.
async fn public_day_latency(
    State(s): State<AppState>,
    Path(slug): Path<String>,
    Query(q): Query<DayLatencyQuery>,
) -> Result<Json<DayLatencyDto>, ApiError> {
    // Resolve the slug + monitor index to a concrete MonitorId without
    // exposing it to the public projection. We re-fetch the StatusPage
    // edge list (one cheap query) rather than calling the full
    // `public_view`, which would do all the per-monitor rollups again.
    let page = s.store().get_status_page_by_slug(&slug).await?;
    let monitor_id = page
        .monitor_ids
        .get(q.monitor_idx)
        .copied()
        .ok_or(ApiError::NotFound)?;

    let rows = s.store().day_hourly_latency(monitor_id, q.date).await?;

    // Dense 24-entry pivot — frontend renders one bar per hour. Hours
    // missing from the sparse rows land as no-data buckets.
    let mut hours: Vec<DayLatencyHour> = (0..24)
        .map(|h| DayLatencyHour {
            hour: h,
            avg_latency_ms: None,
            samples: 0,
        })
        .collect();
    for (h, avg, samples) in rows {
        if (0..24).contains(&h) {
            hours[h as usize] = DayLatencyHour {
                hour: h,
                avg_latency_ms: avg,
                samples,
            };
        }
    }

    Ok(Json(DayLatencyDto { hours }))
}

/// Atom 1.0 feed of incidents for a public status page. RSS/Atom is
/// the universal "subscribe to updates" channel a visitor can drop
/// into any reader (Feedly, NetNewsWire, Slack /feed, etc.) without
/// the operator having to register them as an email subscriber. One
/// entry per incident (active + recently-resolved); each carries the
/// incident's running content + the most recent update so reader-side
/// dedup is sensible.
async fn public_feed_atom(
    State(s): State<AppState>,
    Path(slug): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let page = s.store().status_page_public_view(&slug).await?;
    let xml = render_atom_feed(&page);
    Ok((
        StatusCode::OK,
        [("content-type", "application/atom+xml; charset=utf-8")],
        xml,
    ))
}

/// RSS 2.0 feed companion for readers that don't speak Atom. Same data
/// projection as the Atom feed; the format is the surface difference.
async fn public_feed_rss(
    State(s): State<AppState>,
    Path(slug): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let page = s.store().status_page_public_view(&slug).await?;
    let xml = render_rss_feed(&page);
    Ok((
        StatusCode::OK,
        [("content-type", "application/rss+xml; charset=utf-8")],
        xml,
    ))
}

/// Resolve a `(slug, incident_id)` pair to a concrete incident plus its
/// running updates, enforcing that the incident actually belongs to the
/// named page. A private page (or one whose incident lives elsewhere)
/// 404s — the per-incident feed is only meaningful for an incident the
/// caller could already see on that page. Updates come back oldest-first
/// (the `list_updates` ordering), which the renderers then walk newest-
/// first for the entry/item ordering.
async fn resolve_incident(
    s: &AppState,
    slug: &str,
    incident_id: &str,
) -> Result<(StatusPage, Incident, Vec<IncidentUpdate>), ApiError> {
    let page = s.store().get_status_page_by_slug(slug).await?;
    if page.private {
        return Err(ApiError::NotFound);
    }
    let inc_id = Uuid::from_str(incident_id)
        .map(IncidentId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid incident id".into()))?;
    let incident = s.store().get_incident(inc_id).await?;
    // The incident must live on this page — otherwise a caller could read
    // any incident's thread through any page's slug.
    if incident.status_page_id != page.id {
        return Err(ApiError::NotFound);
    }
    let updates = s.store().list_incident_updates(inc_id).await?;
    Ok((page, incident, updates))
}

/// Atom 1.0 feed scoped to a single incident's update thread. Where the
/// page-level feed emits one entry per incident, this emits one entry per
/// running post: the oldest entry is the incident's initial content, then
/// one entry per `IncidentUpdate`. Entries are newest-first so a reader
/// surfaces the latest post at the top of the thread.
async fn public_incident_feed_atom(
    State(s): State<AppState>,
    Path((slug, incident_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    let (page, incident, updates) = resolve_incident(&s, &slug, &incident_id).await?;
    let xml = render_incident_atom_feed(&page, &incident, &updates);
    Ok((
        StatusCode::OK,
        [("content-type", "application/atom+xml; charset=utf-8")],
        xml,
    ))
}

/// RSS 2.0 companion to the per-incident Atom feed — same projection,
/// different envelope.
async fn public_incident_feed_rss(
    State(s): State<AppState>,
    Path((slug, incident_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    let (page, incident, updates) = resolve_incident(&s, &slug, &incident_id).await?;
    let xml = render_incident_rss_feed(&page, &incident, &updates);
    Ok((
        StatusCode::OK,
        [("content-type", "application/rss+xml; charset=utf-8")],
        xml,
    ))
}

/// XML-text escape: only the five characters the XML spec requires —
/// `&`, `<`, `>`, `"`, `'`. The feed templates wrap user-provided text
/// (incident title + content + update messages) in element bodies, so
/// this is the right escape for CDATA-free content.
fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

/// RFC 3339 / ISO 8601 timestamp formatter for Atom `<updated>` /
/// `<published>` fields. Uses the `time` crate's RFC 3339 well-known
/// format which always emits a `Z` suffix for UTC inputs.
fn rfc3339(t: time::OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| String::new())
}

/// RFC 822 timestamp formatter (the format RSS 2.0 expects). The `time`
/// crate doesn't ship an RFC 822 formatter; format manually.
fn rfc822(t: time::OffsetDateTime) -> String {
    let weekday = match t.weekday() {
        time::Weekday::Monday => "Mon",
        time::Weekday::Tuesday => "Tue",
        time::Weekday::Wednesday => "Wed",
        time::Weekday::Thursday => "Thu",
        time::Weekday::Friday => "Fri",
        time::Weekday::Saturday => "Sat",
        time::Weekday::Sunday => "Sun",
    };
    let month = match t.month() {
        time::Month::January => "Jan",
        time::Month::February => "Feb",
        time::Month::March => "Mar",
        time::Month::April => "Apr",
        time::Month::May => "May",
        time::Month::June => "Jun",
        time::Month::July => "Jul",
        time::Month::August => "Aug",
        time::Month::September => "Sep",
        time::Month::October => "Oct",
        time::Month::November => "Nov",
        time::Month::December => "Dec",
    };
    format!(
        "{weekday}, {day:02} {month} {year:04} {hour:02}:{minute:02}:{second:02} +0000",
        day = t.day(),
        year = t.year(),
        hour = t.hour(),
        minute = t.minute(),
        second = t.second(),
    )
}

fn render_atom_feed(page: &PublicStatusPage) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(2048);
    let _ = writeln!(out, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
    let _ = writeln!(out, "<feed xmlns=\"http://www.w3.org/2005/Atom\">");
    let _ = writeln!(out, "  <title>{}</title>", xml_escape(&page.title));
    let _ = writeln!(
        out,
        "  <id>urn:rampart:status-page:{}</id>",
        xml_escape(&page.slug)
    );
    let _ = writeln!(out, "  <updated>{}</updated>", rfc3339(page.generated_at));

    // Active incidents first (newest activity wins for feed-reader sort
    // order), then resolved-history entries.
    for inc in &page.incidents {
        let last_update_ts = inc
            .updates
            .iter()
            .map(|u| u.posted_at)
            .max()
            .unwrap_or(inc.created_at);
        let _ = writeln!(out, "  <entry>");
        let _ = writeln!(out, "    <title>{}</title>", xml_escape(&inc.title));
        let _ = writeln!(
            out,
            "    <id>urn:rampart:status-page:{}:incident-active:{}</id>",
            xml_escape(&page.slug),
            rfc3339(inc.created_at)
        );
        let _ = writeln!(
            out,
            "    <published>{}</published>",
            rfc3339(inc.created_at)
        );
        let _ = writeln!(out, "    <updated>{}</updated>", rfc3339(last_update_ts));
        let mut body = inc.content.clone();
        for u in &inc.updates {
            body.push_str("\n\n");
            body.push_str(&format!("[{}] ", rfc3339(u.posted_at)));
            body.push_str(&u.message);
        }
        let _ = writeln!(
            out,
            "    <content type=\"text\">{}</content>",
            xml_escape(&body)
        );
        let _ = writeln!(out, "  </entry>");
    }

    for inc in &page.incident_history {
        let _ = writeln!(out, "  <entry>");
        let _ = writeln!(
            out,
            "    <title>{} (resolved)</title>",
            xml_escape(&inc.title)
        );
        let _ = writeln!(
            out,
            "    <id>urn:rampart:status-page:{}:incident-resolved:{}</id>",
            xml_escape(&page.slug),
            rfc3339(inc.created_at)
        );
        let _ = writeln!(
            out,
            "    <published>{}</published>",
            rfc3339(inc.created_at)
        );
        let _ = writeln!(out, "    <updated>{}</updated>", rfc3339(inc.resolved_at));
        let _ = writeln!(
            out,
            "    <content type=\"text\">{}</content>",
            xml_escape(&inc.content)
        );
        let _ = writeln!(out, "  </entry>");
    }

    let _ = writeln!(out, "</feed>");
    out
}

fn render_rss_feed(page: &PublicStatusPage) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(2048);
    let _ = writeln!(out, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
    let _ = writeln!(out, "<rss version=\"2.0\">");
    let _ = writeln!(out, "  <channel>");
    let _ = writeln!(out, "    <title>{}</title>", xml_escape(&page.title));
    let _ = writeln!(
        out,
        "    <description>{}</description>",
        xml_escape(page.description.as_deref().unwrap_or(""))
    );
    let _ = writeln!(out, "    <pubDate>{}</pubDate>", rfc822(page.generated_at));

    for inc in &page.incidents {
        let last_update_ts = inc
            .updates
            .iter()
            .map(|u| u.posted_at)
            .max()
            .unwrap_or(inc.created_at);
        let mut body = inc.content.clone();
        for u in &inc.updates {
            body.push_str("\n\n");
            body.push_str(&format!("[{}] ", rfc3339(u.posted_at)));
            body.push_str(&u.message);
        }
        let _ = writeln!(out, "    <item>");
        let _ = writeln!(out, "      <title>{}</title>", xml_escape(&inc.title));
        let _ = writeln!(
            out,
            "      <description>{}</description>",
            xml_escape(&body)
        );
        let _ = writeln!(out, "      <pubDate>{}</pubDate>", rfc822(last_update_ts));
        let _ = writeln!(
            out,
            "      <guid isPermaLink=\"false\">urn:rampart:status-page:{}:incident-active:{}</guid>",
            xml_escape(&page.slug),
            rfc3339(inc.created_at)
        );
        let _ = writeln!(out, "    </item>");
    }

    for inc in &page.incident_history {
        let _ = writeln!(out, "    <item>");
        let _ = writeln!(
            out,
            "      <title>{} (resolved)</title>",
            xml_escape(&inc.title)
        );
        let _ = writeln!(
            out,
            "      <description>{}</description>",
            xml_escape(&inc.content)
        );
        let _ = writeln!(out, "      <pubDate>{}</pubDate>", rfc822(inc.resolved_at));
        let _ = writeln!(
            out,
            "      <guid isPermaLink=\"false\">urn:rampart:status-page:{}:incident-resolved:{}</guid>",
            xml_escape(&page.slug),
            rfc3339(inc.created_at)
        );
        let _ = writeln!(out, "    </item>");
    }

    let _ = writeln!(out, "  </channel>");
    let _ = writeln!(out, "</rss>");
    out
}

/// Atom 1.0 feed for one incident's thread. The feed title/id is the
/// incident itself; entries are the running posts newest-first — every
/// `IncidentUpdate` plus, as the oldest entry, the incident's initial
/// content. `feed.updated` tracks the most recent post (latest update, or
/// the incident creation when there are no updates yet).
fn render_incident_atom_feed(
    page: &StatusPage,
    inc: &Incident,
    updates: &[IncidentUpdate],
) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(2048);
    let last_ts = updates
        .iter()
        .map(|u| u.posted_at)
        .max()
        .unwrap_or(inc.created_at);
    let _ = writeln!(out, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
    let _ = writeln!(out, "<feed xmlns=\"http://www.w3.org/2005/Atom\">");
    let _ = writeln!(out, "  <title>{}</title>", xml_escape(&inc.title));
    let _ = writeln!(
        out,
        "  <id>urn:rampart:status-page:{}:incident:{}</id>",
        xml_escape(&page.slug),
        inc.id.0
    );
    let _ = writeln!(out, "  <updated>{}</updated>", rfc3339(last_ts));

    // Running updates newest-first.
    for u in updates.iter().rev() {
        let _ = writeln!(out, "  <entry>");
        let _ = writeln!(out, "    <title>{}</title>", xml_escape(&inc.title));
        let _ = writeln!(
            out,
            "    <id>urn:rampart:incident:{}:update:{}</id>",
            inc.id.0, u.id.0
        );
        let _ = writeln!(out, "    <published>{}</published>", rfc3339(u.posted_at));
        let _ = writeln!(out, "    <updated>{}</updated>", rfc3339(u.posted_at));
        let _ = writeln!(
            out,
            "    <content type=\"text\">{}</content>",
            xml_escape(&u.message)
        );
        let _ = writeln!(out, "  </entry>");
    }

    // The original incident content as the oldest entry.
    let _ = writeln!(out, "  <entry>");
    let _ = writeln!(out, "    <title>{}</title>", xml_escape(&inc.title));
    let _ = writeln!(out, "    <id>urn:rampart:incident:{}:opened</id>", inc.id.0);
    let _ = writeln!(
        out,
        "    <published>{}</published>",
        rfc3339(inc.created_at)
    );
    let _ = writeln!(out, "    <updated>{}</updated>", rfc3339(inc.created_at));
    let _ = writeln!(
        out,
        "    <content type=\"text\">{}</content>",
        xml_escape(&inc.content)
    );
    let _ = writeln!(out, "  </entry>");

    let _ = writeln!(out, "</feed>");
    out
}

/// RSS 2.0 feed for one incident's thread — same projection as
/// [`render_incident_atom_feed`], newest-first items.
fn render_incident_rss_feed(
    page: &StatusPage,
    inc: &Incident,
    updates: &[IncidentUpdate],
) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(2048);
    let last_ts = updates
        .iter()
        .map(|u| u.posted_at)
        .max()
        .unwrap_or(inc.created_at);
    let _ = writeln!(out, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
    let _ = writeln!(out, "<rss version=\"2.0\">");
    let _ = writeln!(out, "  <channel>");
    let _ = writeln!(out, "    <title>{}</title>", xml_escape(&inc.title));
    let _ = writeln!(
        out,
        "    <description>{}</description>",
        xml_escape(&inc.content)
    );
    let _ = writeln!(out, "    <link>{}</link>", xml_escape(&page.slug));
    let _ = writeln!(out, "    <pubDate>{}</pubDate>", rfc822(last_ts));

    for u in updates.iter().rev() {
        let _ = writeln!(out, "    <item>");
        let _ = writeln!(out, "      <title>{}</title>", xml_escape(&inc.title));
        let _ = writeln!(
            out,
            "      <description>{}</description>",
            xml_escape(&u.message)
        );
        let _ = writeln!(out, "      <pubDate>{}</pubDate>", rfc822(u.posted_at));
        let _ = writeln!(
            out,
            "      <guid isPermaLink=\"false\">urn:rampart:incident:{}:update:{}</guid>",
            inc.id.0, u.id.0
        );
        let _ = writeln!(out, "    </item>");
    }

    let _ = writeln!(out, "    <item>");
    let _ = writeln!(out, "      <title>{}</title>", xml_escape(&inc.title));
    let _ = writeln!(
        out,
        "      <description>{}</description>",
        xml_escape(&inc.content)
    );
    let _ = writeln!(out, "      <pubDate>{}</pubDate>", rfc822(inc.created_at));
    let _ = writeln!(
        out,
        "      <guid isPermaLink=\"false\">urn:rampart:incident:{}:opened</guid>",
        inc.id.0
    );
    let _ = writeln!(out, "    </item>");

    let _ = writeln!(out, "  </channel>");
    let _ = writeln!(out, "</rss>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_domain_accepts_plain_hostname() {
        assert!(validate_custom_domain(Some("status.acme.com")).is_ok());
        assert!(validate_custom_domain(Some("status-1.acme-corp.io")).is_ok());
        assert!(validate_custom_domain(None).is_ok());
    }

    #[test]
    fn custom_domain_rejects_bad_chars_and_length() {
        assert!(validate_custom_domain(Some("Status.Acme.com")).is_err()); // uppercase
        assert!(validate_custom_domain(Some("status acme.com")).is_err()); // space
        assert!(validate_custom_domain(Some("status_acme.com")).is_err()); // underscore
        assert!(validate_custom_domain(Some("")).is_err()); // empty
        let too_long = format!("{}.com", "a".repeat(260));
        assert!(validate_custom_domain(Some(&too_long)).is_err());
    }

    #[test]
    fn logo_url_accepts_http_and_data_uri() {
        assert!(validate_logo_url(Some("https://cdn.acme.com/logo.png")).is_ok());
        assert!(validate_logo_url(Some("http://acme.com/logo.svg")).is_ok());
        assert!(validate_logo_url(Some("data:image/png;base64,iVBORw0KGgo=")).is_ok());
        assert!(validate_logo_url(None).is_ok());
        assert!(validate_logo_url(Some("")).is_ok());
    }

    #[test]
    fn logo_url_rejects_non_url_and_oversized_data_uri() {
        assert!(validate_logo_url(Some("ftp://acme.com/logo.png")).is_err());
        assert!(validate_logo_url(Some("just-a-string")).is_err());
        assert!(validate_logo_url(Some("https://")).is_err());
        // A data: URI just over the 512 KB cap.
        let big = format!(
            "data:image/png;base64,{}",
            "A".repeat(MAX_LOGO_DATA_URI_BYTES)
        );
        assert!(validate_logo_url(Some(&big)).is_err());
    }
}
