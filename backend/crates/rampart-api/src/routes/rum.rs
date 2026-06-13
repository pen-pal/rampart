//! Real User Monitoring — beacon ingest + read views.
//!
//! Ingest (root `/rum`, public — beacons come from arbitrary browsers):
//!   POST /rum/v1/events  — one page-view beacon (sent via navigator.sendBeacon)
//!   GET  /rum/snippet.js  — the self-installing collector script
//! Read (`/v1/rum`, editor/readonly):
//!   GET /v1/rum/summary | /pages | /apps

use crate::error::ApiError;
use crate::state::AppState;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use rampart_core::rum::{RumBeacon, RumPage, RumVitals};
use serde::Deserialize;

/// Root-level public ingest + snippet.
pub fn ingest_router() -> Router<AppState> {
    Router::new()
        .route("/v1/events", post(ingest))
        .route("/snippet.js", get(snippet))
}

/// `/v1/rum` read views.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/summary", get(summary))
        .route("/pages", get(pages))
        .route("/apps", get(apps))
}

#[derive(Deserialize)]
struct BeaconQuery {
    /// Optional ingest token, forwarded by the snippet's `data-token`. The
    /// beacon path uses a query param because `navigator.sendBeacon` can't
    /// set request headers. This token is necessarily public (it ships in
    /// the browser snippet) — it's an anti-abuse gate, not a secret.
    k: Option<String>,
}

/// Accept a beacon. Body is JSON (sent as text/plain by sendBeacon, so we
/// parse the raw bytes regardless of content-type; gzip/deflate is inflated
/// for the rare client that compresses). A malformed or empty beacon is
/// silently accepted (204) — browsers ignore the response and we don't want
/// ingest noise. If a telemetry token is configured it is enforced first.
async fn ingest(
    State(s): State<AppState>,
    Query(q): Query<BeaconQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    if crate::ingest_util::require_telemetry_token(s.pool(), &headers, q.k.as_deref())
        .await
        .is_err()
    {
        return StatusCode::UNAUTHORIZED;
    }
    let raw = match crate::ingest_util::decompress(&headers, &body) {
        Ok(r) => r,
        Err(_) => return StatusCode::NO_CONTENT,
    };
    if let Ok(beacon) = serde_json::from_slice::<RumBeacon>(&raw) {
        if let Some(clean) = beacon.clean() {
            let _ = rampart_db::rum::insert_event(s.pool(), &clean).await;
        }
    }
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
struct RumQuery {
    app: Option<String>,
    hours: Option<i32>,
}

async fn summary(
    State(s): State<AppState>,
    Query(q): Query<RumQuery>,
) -> Result<Json<RumVitals>, ApiError> {
    Ok(Json(
        rampart_db::rum::summary(s.pool(), q.app.as_deref(), q.hours.unwrap_or(24)).await?,
    ))
}

async fn pages(
    State(s): State<AppState>,
    Query(q): Query<RumQuery>,
) -> Result<Json<Vec<RumPage>>, ApiError> {
    Ok(Json(
        rampart_db::rum::pages(s.pool(), q.app.as_deref(), q.hours.unwrap_or(24)).await?,
    ))
}

async fn apps(State(s): State<AppState>) -> Result<Json<Vec<String>>, ApiError> {
    Ok(Json(rampart_db::rum::apps(s.pool()).await?))
}

/// The browser RUM collector. Self-configures from its own `<script>` tag
/// (`data-app`, optional `data-endpoint`), gathers Core Web Vitals via
/// PerformanceObserver + Navigation Timing, and sends one beacon on page hide.
async fn snippet() -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        SNIPPET,
    )
}

const SNIPPET: &str = r#"(function(){
  var s=document.currentScript, app=(s&&s.getAttribute('data-app'))||'web';
  var base=(s&&s.getAttribute('data-endpoint'))||(s&&new URL(s.src).origin)||location.origin;
  var tok=(s&&s.getAttribute('data-token'))||'';
  var ep=base.replace(/\/$/,'')+'/rum/v1/events'+(tok?('?k='+encodeURIComponent(tok)):'');
  var m={}, sid=Math.random().toString(36).slice(2);
  function obs(type,cb){try{new PerformanceObserver(cb).observe({type:type,buffered:true});}catch(e){}}
  obs('largest-contentful-paint',function(l){var e=l.getEntries();if(e.length)m.lcp=e[e.length-1].startTime;});
  var cls=0; obs('layout-shift',function(l){l.getEntries().forEach(function(e){if(!e.hadRecentInput)cls+=e.value;});m.cls=cls;});
  var inp=0; obs('event',function(l){l.getEntries().forEach(function(e){if(e.duration>inp)inp=e.duration;});m.inp=inp;});
  function nav(){var n=performance.getEntriesByType('navigation')[0];if(n){m.ttfb=n.responseStart;m.load=n.loadEventEnd||n.duration;}performance.getEntriesByType('paint').forEach(function(e){if(e.name==='first-contentful-paint')m.fcp=e.startTime;});}
  var sent=false;
  function send(){if(sent)return;nav();if(!Object.keys(m).length)return;sent=true;
    var body=JSON.stringify({app:app,url:location.pathname,session:sid,ua:navigator.userAgent,metrics:m});
    try{navigator.sendBeacon(ep,body);}catch(e){}}
  addEventListener('visibilitychange',function(){if(document.visibilityState==='hidden')send();});
  addEventListener('pagehide',send);
})();
"#;
