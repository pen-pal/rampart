//! Real User Monitoring — beacon ingest + read views.
//!
//! Ingest (root `/rum`, public — beacons come from arbitrary browsers):
//!   POST /rum/v1/events  — one page-view beacon (sent via navigator.sendBeacon)
//!   POST /rum/v1/errors  — one browser JS error → recorded in the error tier
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
        .route("/v1/errors", post(ingest_error))
        .route("/snippet.js", get(snippet))
}

/// `/v1/rum` read views.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/summary", get(summary))
        .route("/pages", get(pages))
        .route("/apps", get(apps))
        .route("/traced", get(traced))
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

/// A browser JavaScript error captured by the RUM snippet's `window.onerror`
/// / `unhandledrejection` handlers. Cross-tier: these are funneled into the
/// error-tracking tier under a project auto-named after the beacon's `app`, so
/// front-end exceptions group + alert exactly like backend SDK errors.
#[derive(Deserialize)]
struct RumError {
    app: Option<String>,
    message: Option<String>,
    /// Error constructor name, e.g. `TypeError`. Defaults to `Error`.
    kind: Option<String>,
    /// Page URL where it fired — becomes the issue culprit.
    url: Option<String>,
    /// Raw JS stack string (unparsed); kept in the event for context.
    stack: Option<String>,
}

/// Accept a browser error beacon and record it in the error tier. Same
/// public/token/compression contract as the metrics beacon. Best-effort: a
/// malformed body or empty message is accepted (204) without recording.
async fn ingest_error(
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
    let Ok(err) = serde_json::from_slice::<RumError>(&raw) else {
        return StatusCode::NO_CONTENT;
    };
    let message = err.message.unwrap_or_default();
    if message.trim().is_empty() {
        return StatusCode::NO_CONTENT;
    }
    let app = err
        .app
        .as_deref()
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .unwrap_or("web")
        .to_string();

    // Provision (or reuse) the per-app browser project, then group + store the
    // error through the same path as SDK ingest.
    let project = match rampart_db::error_tracking::find_or_create_by_name(s.pool(), &app).await {
        Ok(p) => p,
        Err(_) => return StatusCode::NO_CONTENT,
    };

    let kind = err.kind.unwrap_or_else(|| "Error".to_string());
    let sentry = serde_json::json!({
        "level": "error",
        "platform": "javascript",
        "transaction": err.url,
        "message": message,
        "exception": { "values": [ {
            "type": kind,
            "value": message,
        } ] },
        "contexts": { "browser_error": {
            "url": err.url,
            "stack": err.stack,
        } },
    });
    let parsed = rampart_core::error_tracking::ParsedEvent::from_sentry_json(sentry);
    let outcome =
        match rampart_db::error_tracking::record_event(s.pool(), project.id, &parsed).await {
            Ok(o) => o,
            Err(_) => return StatusCode::NO_CONTENT,
        };

    // Alert on new / regressed issues only, off the request path (mirrors the
    // Sentry ingest route).
    if (outcome.is_new || outcome.regressed) && !project.alert_channel_ids.is_empty() {
        let pool = s.pool().clone();
        let channels = project.alert_channel_ids.clone();
        let name = project.name.clone();
        let title = outcome.title.clone();
        let regressed = outcome.regressed;
        tokio::spawn(async move {
            rampart_notifier::service::dispatch_error_alert(
                &pool, &name, &channels, regressed, &title,
            )
            .await;
        });
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

async fn traced(
    State(s): State<AppState>,
    Query(q): Query<RumQuery>,
) -> Result<Json<Vec<rampart_core::rum::RumTracedLoad>>, ApiError> {
    Ok(Json(
        rampart_db::rum::recent_traced(s.pool(), q.app.as_deref(), q.hours.unwrap_or(24), 50)
            .await?,
    ))
}

/// The browser RUM collector. Self-configures from its own `<script>` tag
/// (`data-app`, optional `data-endpoint`, optional `data-token`), gathers Core
/// Web Vitals via PerformanceObserver + Navigation Timing (one beacon on page
/// hide), and captures uncaught errors + unhandled promise rejections,
/// forwarding them to the error tier (`/rum/v1/errors`).
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
  var qs=(tok?('?k='+encodeURIComponent(tok)):'');
  var ep=base.replace(/\/$/,'')+'/rum/v1/events'+qs;
  var eep=base.replace(/\/$/,'')+'/rum/v1/errors'+qs;
  var m={}, sid=Math.random().toString(36).slice(2);
  function err(kind,msg,stack){if(!msg)return;
    try{navigator.sendBeacon(eep,JSON.stringify({app:app,kind:kind,message:String(msg).slice(0,500),url:location.href,stack:stack?String(stack).slice(0,4000):null}));}catch(e){}}
  addEventListener('error',function(e){err((e.error&&e.error.name)||'Error',(e.error&&e.error.message)||e.message,e.error&&e.error.stack);});
  addEventListener('unhandledrejection',function(e){var r=e.reason||{};err((r&&r.name)||'UnhandledRejection',(r&&r.message)||String(r),r&&r.stack);});
  function obs(type,cb){try{new PerformanceObserver(cb).observe({type:type,buffered:true});}catch(e){}}
  obs('largest-contentful-paint',function(l){var e=l.getEntries();if(e.length)m.lcp=e[e.length-1].startTime;});
  var cls=0; obs('layout-shift',function(l){l.getEntries().forEach(function(e){if(!e.hadRecentInput)cls+=e.value;});m.cls=cls;});
  var inp=0; obs('event',function(l){l.getEntries().forEach(function(e){if(e.duration>inp)inp=e.duration;});m.inp=inp;});
  function nav(){var n=performance.getEntriesByType('navigation')[0];if(n){m.ttfb=n.responseStart;m.load=n.loadEventEnd||n.duration;}performance.getEntriesByType('paint').forEach(function(e){if(e.name==='first-contentful-paint')m.fcp=e.startTime;});}
  var sent=false;
  // Best-effort backend trace id for RUM->trace correlation: an explicit
  // window.__rampartTraceId wins, else the trace-id field of a <meta
  // name="traceparent"> (W3C: 00-<traceid>-<spanid>-<flags>).
  function tid(){try{if(window.__rampartTraceId)return String(window.__rampartTraceId);
    var mt=document.querySelector('meta[name=traceparent]');
    if(mt){var p=(mt.content||'').split('-');if(p.length>=2&&p[1]&&/^[0-9a-f]+$/i.test(p[1]))return p[1];}}catch(e){}return null;}
  function send(){if(sent)return;nav();if(!Object.keys(m).length)return;sent=true;
    var body=JSON.stringify({app:app,url:location.pathname,session:sid,ua:navigator.userAgent,trace_id:tid(),metrics:m});
    try{navigator.sendBeacon(ep,body);}catch(e){}}
  addEventListener('visibilitychange',function(){if(document.visibilityState==='hidden')send();});
  addEventListener('pagehide',send);
})();
"#;
