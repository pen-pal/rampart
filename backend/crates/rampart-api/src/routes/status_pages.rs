//! `/v1/status-pages` (admin CRUD) and `/v1/public/status-pages/:slug`
//! (unauthenticated public view).
//!
//! The admin half lives behind the session middleware just like every
//! other /v1 route. The public half intentionally does NOT — the page
//! is meant to be linked from external sites.

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use rampart_core::ids::StatusPageId;
use rampart_core::status_page::{NewStatusPage, PublicStatusPage, StatusPage, UpdateStatusPage};
use rampart_db::users::User;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;
use validator::Validate;

pub fn admin_router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", get(get_one).patch(update).delete(remove))
}

pub fn public_router() -> Router<AppState> {
    Router::new()
        .route("/{slug}", get(public_view))
        .route("/{slug}/feed.atom", get(public_feed_atom))
        .route("/{slug}/feed.rss", get(public_feed_rss))
        .route("/{slug}/day-latency", get(public_day_latency))
}

fn parse(id: &str) -> Result<StatusPageId, ApiError> {
    Uuid::from_str(id)
        .map(StatusPageId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid status page id".into()))
}

async fn list(State(s): State<AppState>) -> Result<Json<Vec<StatusPage>>, ApiError> {
    Ok(Json(rampart_db::status_pages::list(s.pool()).await?))
}

async fn get_one(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<StatusPage>, ApiError> {
    Ok(Json(
        rampart_db::status_pages::get(s.pool(), parse(&id)?).await?,
    ))
}

async fn create(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Json(input): Json<NewStatusPage>,
) -> Result<(StatusCode, Json<StatusPage>), ApiError> {
    input
        .validate()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let slug = input.slug.clone();
    let p = rampart_db::status_pages::create(s.pool(), input).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "status_page.create",
        "status_page",
        Some(p.id.0),
        Some(serde_json::json!({ "slug": slug })),
    )
    .await;
    Ok((StatusCode::CREATED, Json(p)))
}

async fn update(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<UpdateStatusPage>,
) -> Result<Json<StatusPage>, ApiError> {
    Ok(Json(
        rampart_db::status_pages::update(s.pool(), parse(&id)?, input).await?,
    ))
}

async fn remove(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let page_id = parse(&id)?;
    rampart_db::status_pages::delete(s.pool(), page_id).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "status_page.delete",
        "status_page",
        Some(page_id.0),
        None,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

async fn public_view(
    State(s): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<PublicStatusPage>, ApiError> {
    Ok(Json(
        rampart_db::status_pages::public_view(s.pool(), &slug).await?,
    ))
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
    let page = rampart_db::status_pages::get_by_slug(s.pool(), &slug).await?;
    let monitor_id = page
        .monitor_ids
        .get(q.monitor_idx)
        .copied()
        .ok_or(ApiError::NotFound)?;

    let rows = rampart_db::heartbeats::day_hourly_latency(s.pool(), monitor_id, q.date).await?;

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
    let page = rampart_db::status_pages::public_view(s.pool(), &slug).await?;
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
    let page = rampart_db::status_pages::public_view(s.pool(), &slug).await?;
    let xml = render_rss_feed(&page);
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
