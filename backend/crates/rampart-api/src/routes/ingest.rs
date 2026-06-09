//! Inbound alert ingestion + ingest-token management.
//!
//! Three slices:
//!
//! - Public, token-authed webhook receiver:
//!   POST /v1/public/ingest/alertmanager/{token}
//!   Accepts a Prometheus Alertmanager webhook payload and turns each
//!   contained alert into a status-page incident (firing → create,
//!   resolved → resolve the matching open incident). The token in the URL
//!   IS the auth — there is no session.
//!
//! - Page-scoped admin management (session-gated, mounted under
//!   /v1/status-pages):
//!   GET  /v1/status-pages/{id}/ingest-tokens — list
//!   POST /v1/status-pages/{id}/ingest-tokens — mint a new token
//!
//! - Top-level admin revoke (session-gated, mounted under /v1):
//!   DELETE /v1/ingest-tokens/{id} — revoke
//!
//! Incident logic is NOT duplicated here — we reuse
//! `rampart_db::incidents::{create, list_active, resolve}`.

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use rampart_core::ids::{IngestTokenId, StatusPageId};
use rampart_core::ingest_token::{IngestToken, NewIngestToken};
use rampart_core::IncidentStyle;
use rampart_db::incidents::NewIncident;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use time::OffsetDateTime;
use uuid::Uuid;

// ---- routers -------------------------------------------------------------

/// Public webhook receiver. Nested under `/v1/public` in routes/mod.rs.
pub fn public_router() -> Router<AppState> {
    Router::new().route("/ingest/alertmanager/{token}", post(alertmanager))
}

/// Page-scoped token management. Merged into the `/v1/status-pages` admin nest.
pub fn page_router() -> Router<AppState> {
    Router::new().route("/{id}/ingest-tokens", get(list_tokens).post(create_token))
}

/// Top-level revoke. Nested at `/v1/ingest-tokens`.
pub fn token_router() -> Router<AppState> {
    Router::new().route("/{id}", axum::routing::delete(revoke_token))
}

fn parse_page(s: &str) -> Result<StatusPageId, ApiError> {
    Uuid::from_str(s)
        .map(StatusPageId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid status page id".into()))
}
fn parse_token_id(s: &str) -> Result<IngestTokenId, ApiError> {
    Uuid::from_str(s)
        .map(IngestTokenId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid ingest token id".into()))
}

// ---- admin: token management --------------------------------------------

async fn list_tokens(
    State(s): State<AppState>,
    Path(page): Path<String>,
) -> Result<Json<Vec<IngestToken>>, ApiError> {
    Ok(Json(
        rampart_db::ingest_tokens::list_for_page(s.pool(), parse_page(&page)?).await?,
    ))
}

async fn create_token(
    State(s): State<AppState>,
    Path(page): Path<String>,
    Json(input): Json<NewIngestToken>,
) -> Result<(StatusCode, Json<IngestToken>), ApiError> {
    let page_id = parse_page(&page)?;
    let tok = rampart_db::ingest_tokens::create(s.pool(), page_id, input).await?;
    Ok((StatusCode::CREATED, Json(tok)))
}

async fn revoke_token(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    rampart_db::ingest_tokens::delete(s.pool(), parse_token_id(&id)?).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---- public: Alertmanager webhook ---------------------------------------

/// Minimal shape of the Alertmanager webhook payload. We only read the
/// fields we map onto incidents; everything else is ignored via serde's
/// default field-dropping. `#[serde(default)]` keeps us lenient about
/// missing optional sections so a partial/older payload still parses.
#[derive(Debug, Deserialize)]
struct AlertmanagerPayload {
    #[serde(default)]
    alerts: Vec<AlertmanagerAlert>,
}

#[derive(Debug, Deserialize)]
struct AlertmanagerAlert {
    #[serde(default)]
    status: String,
    #[serde(default)]
    labels: std::collections::HashMap<String, String>,
    #[serde(default)]
    annotations: std::collections::HashMap<String, String>,
}

#[derive(Debug, Serialize)]
struct IngestSummary {
    created: usize,
    resolved: usize,
}

async fn alertmanager(
    State(s): State<AppState>,
    Path(token): Path<String>,
    Json(payload): Json<AlertmanagerPayload>,
) -> Result<(StatusCode, Json<IngestSummary>), ApiError> {
    // Resolve the token → 404 for anything unknown. This is the whole auth
    // check: possession of a valid token authorizes ingest to its page.
    let tok = rampart_db::ingest_tokens::find_by_token(s.pool(), &token)
        .await
        .map_err(|_| ApiError::NotFound)?;
    // Best-effort last-used bump — don't fail the ingest on a touch error.
    let _ = rampart_db::ingest_tokens::touch_last_used(s.pool(), tok.id).await;

    let page = tok.status_page_id;
    let now = OffsetDateTime::now_utc();
    let mut created = 0usize;
    let mut resolved = 0usize;

    for alert in &payload.alerts {
        let alertname = alert
            .labels
            .get("alertname")
            .map(String::as_str)
            .unwrap_or("")
            .trim();
        // Title: prefer the alertname (it's our resolve dedup key), fall
        // back to the summary annotation. Skip alerts that carry neither.
        let title = if !alertname.is_empty() {
            alertname.to_string()
        } else {
            match alert.annotations.get("summary") {
                Some(sumtxt) if !sumtxt.trim().is_empty() => sumtxt.trim().to_string(),
                _ => continue,
            }
        };

        match alert.status.as_str() {
            "resolved" => {
                // Match the most recent active incident on this page whose
                // title equals our dedup key, and resolve it. list_active
                // already returns newest-first.
                let active = rampart_db::incidents::list_active(s.pool(), page).await?;
                if let Some(inc) = active.into_iter().find(|i| i.title == title) {
                    rampart_db::incidents::resolve(s.pool(), inc.id, now).await?;
                    resolved += 1;
                }
            }
            // Treat anything that isn't "resolved" as firing — Alertmanager
            // sends "firing", but a missing/unknown status shouldn't drop
            // the alert silently.
            _ => {
                let content = alert
                    .annotations
                    .get("description")
                    .cloned()
                    .or_else(|| alert.annotations.get("summary").cloned())
                    .unwrap_or_default();
                let style = style_for_severity(alert.labels.get("severity").map(String::as_str));
                let new = NewIncident {
                    title,
                    content,
                    style,
                    pinned: true,
                };
                rampart_db::incidents::create(s.pool(), page, None, new).await?;
                created += 1;
            }
        }
    }

    Ok((
        StatusCode::ACCEPTED,
        Json(IngestSummary { created, resolved }),
    ))
}

fn style_for_severity(severity: Option<&str>) -> IncidentStyle {
    match severity.map(str::to_ascii_lowercase).as_deref() {
        Some("critical") => IncidentStyle::Danger,
        Some("warning") => IncidentStyle::Warning,
        _ => IncidentStyle::Info,
    }
}
