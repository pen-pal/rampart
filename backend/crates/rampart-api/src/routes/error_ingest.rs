//! Sentry-compatible error ingest (DSN-keyed, unauthenticated session-wise).
//!
//! Mounted at the root `/api` surface so official Sentry SDKs work by pointing
//! their DSN (`https://<public_key>@<host>/<project_id>`) at Rampart:
//!   POST /api/{project_id}/envelope/   — modern envelope transport
//!   POST /api/{project_id}/store/      — legacy single-event transport
//!
//! Auth is the DSN public key (in the `X-Sentry-Auth` header or `?sentry_key=`),
//! matched against the project — Sentry's model, where the key is public and
//! protection is the per-project body cap, not key secrecy. We parse the
//! `event` item and accept-and-ignore other item types (transaction, session).

use crate::error::ApiError;
use crate::state::AppState;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};
use rampart_core::error_tracking::ParsedEvent;
use rampart_core::ids::ErrorProjectId;
use serde::Deserialize;
use serde_json::Value;
use std::io::Read;
use std::str::FromStr;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/{project_id}/envelope/", post(ingest_envelope))
        .route("/{project_id}/envelope", post(ingest_envelope))
        .route("/{project_id}/store/", post(ingest_store))
        .route("/{project_id}/store", post(ingest_store))
}

#[derive(Deserialize)]
struct IngestQuery {
    sentry_key: Option<String>,
}

async fn ingest_envelope(
    State(s): State<AppState>,
    Path(project_id): Path<String>,
    Query(q): Query<IngestQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    ingest(s, project_id, q, headers, body, true).await
}

async fn ingest_store(
    State(s): State<AppState>,
    Path(project_id): Path<String>,
    Query(q): Query<IngestQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    ingest(s, project_id, q, headers, body, false).await
}

async fn ingest(
    s: AppState,
    project_id: String,
    q: IngestQuery,
    headers: HeaderMap,
    body: Bytes,
    envelope: bool,
) -> Result<Json<Value>, ApiError> {
    let pid = Uuid::from_str(&project_id)
        .map(ErrorProjectId::from_uuid)
        .map_err(|_| ApiError::NotFound)?;

    // DSN key auth. A missing project and a bad key both return 404 so ingest
    // doesn't leak which project ids exist.
    let project = rampart_db::error_tracking::get_opt(s.pool(), pid)
        .await?
        .ok_or(ApiError::NotFound)?;
    let key = sentry_key(&headers, &q).ok_or(ApiError::NotFound)?;
    if key != project.public_key {
        return Err(ApiError::NotFound);
    }

    let raw = decompress(&headers, &body)?;
    let event_json = if envelope {
        extract_envelope_event(&raw)
    } else {
        serde_json::from_slice::<Value>(&raw).ok()
    };
    let Some(event_json) = event_json else {
        // No event item (e.g. a session-only envelope) — accept silently so
        // the SDK doesn't retry, but record nothing.
        return Ok(Json(
            serde_json::json!({ "id": Uuid::nil().simple().to_string() }),
        ));
    };

    let parsed = ParsedEvent::from_sentry_json(event_json);
    let outcome = rampart_db::error_tracking::record_event(s.pool(), pid, &parsed).await?;

    // Alert on new / regressed issues only, off the request path.
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

    Ok(Json(
        serde_json::json!({ "id": outcome.event_id.simple().to_string() }),
    ))
}

/// Pull the DSN public key from `?sentry_key=` (browser transport) or the
/// `X-Sentry-Auth` header (`Sentry sentry_key=<k>, sentry_version=7, …`).
fn sentry_key(headers: &HeaderMap, q: &IngestQuery) -> Option<String> {
    if let Some(k) = &q.sentry_key {
        if !k.is_empty() {
            return Some(k.clone());
        }
    }
    let auth = headers.get("x-sentry-auth").and_then(|v| v.to_str().ok())?;
    auth.split([',', ' '])
        .find_map(|p| p.trim().strip_prefix("sentry_key=").map(|s| s.to_string()))
}

/// Decode the body per `Content-Encoding` (Sentry SDKs gzip by default).
fn decompress(headers: &HeaderMap, body: &[u8]) -> Result<Vec<u8>, ApiError> {
    let enc = headers
        .get("content-encoding")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    if enc.contains("gzip") {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(body)
            .read_to_end(&mut out)
            .map_err(|_| ApiError::BadRequest("invalid gzip body".into()))?;
        Ok(out)
    } else if enc.contains("deflate") {
        let mut out = Vec::new();
        flate2::read::ZlibDecoder::new(body)
            .read_to_end(&mut out)
            .map_err(|_| ApiError::BadRequest("invalid deflate body".into()))?;
        Ok(out)
    } else {
        Ok(body.to_vec())
    }
}

/// Find the first `event` item in a Sentry envelope and return its payload.
///
/// Envelope = newline-delimited: an envelope header line, then repeating
/// (item-header line, item-payload line) pairs. We pair lines and pick the
/// payload whose preceding header has `"type":"event"`. (We read compact
/// single-line item payloads, which is what the official SDKs send; the
/// optional `length`-framed form is not handled in v1.)
fn extract_envelope_event(raw: &[u8]) -> Option<Value> {
    let text = std::str::from_utf8(raw).ok()?;
    let mut lines = text.split('\n').filter(|l| !l.is_empty());
    let _envelope_header = lines.next()?; // discard
    loop {
        let header_line = lines.next()?;
        let payload_line = lines.next()?;
        let header: Value = serde_json::from_str(header_line).ok()?;
        if header.get("type").and_then(|t| t.as_str()) == Some("event") {
            return serde_json::from_str(payload_line).ok();
        }
        // else: a non-event item (transaction/session/attachment) — skip its
        // payload (already consumed) and continue.
    }
}
