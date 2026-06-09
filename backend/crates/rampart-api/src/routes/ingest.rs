//! Inbound alert ingestion + ingest-token management.
//!
//! Three slices:
//!
//! - Public, token-authed webhook receivers:
//!   POST /v1/public/ingest/alertmanager/{token}  (Prometheus Alertmanager)
//!   POST /v1/public/ingest/grafana/{token}        (Grafana unified alerting)
//!   POST /v1/public/ingest/datadog/{token}        (Datadog webhook)
//!   Each accepts that vendor's webhook payload and turns the contained
//!   alert(s) into status-page incidents (firing → create, resolved →
//!   resolve the matching open incident). The token in the URL IS the auth
//!   — there is no session. All three normalize their payload into the same
//!   `NormalizedAlert` shape and funnel through `apply_alert`, so the
//!   create-or-resolve logic lives in exactly one place.
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
//! `rampart_db::incidents::{create, resolve, find_active_by_dedup_key}`.

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

/// Public webhook receivers. Nested under `/v1/public` in routes/mod.rs.
pub fn public_router() -> Router<AppState> {
    Router::new()
        .route("/ingest/alertmanager/{token}", post(alertmanager))
        .route("/ingest/grafana/{token}", post(grafana))
        .route("/ingest/datadog/{token}", post(datadog))
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

// ---- shared ingest core --------------------------------------------------

#[derive(Debug, Serialize)]
struct IngestSummary {
    created: usize,
    resolved: usize,
}

/// What every vendor parser boils its payload down to. `apply_alert` is the
/// single create-or-resolve implementation; the per-vendor handlers are
/// nothing but payload parsers that emit these.
enum AlertAction {
    Create,
    Resolve,
}

struct NormalizedAlert {
    action: AlertAction,
    title: String,
    content: String,
    style: IncidentStyle,
    /// Stable per-alert key. On create it is stored on the incident; on
    /// resolve it locates the incident to close.
    dedup_key: String,
}

/// Resolve the ingest token in the URL → the status page it authorizes.
/// 404 for anything unknown — possession of a valid token is the entire
/// auth check.
async fn page_for_token(s: &AppState, token: &str) -> Result<StatusPageId, ApiError> {
    let tok = rampart_db::ingest_tokens::find_by_token(s.pool(), token)
        .await
        .map_err(|_| ApiError::NotFound)?;
    // Best-effort last-used bump — don't fail the ingest on a touch error.
    let _ = rampart_db::ingest_tokens::touch_last_used(s.pool(), tok.id).await;
    Ok(tok.status_page_id)
}

/// Apply one normalized alert to a page. Create opens an incident carrying
/// the dedup key; resolve closes the active incident on the page with the
/// matching key (exact match — no fragile title comparison). Returns the
/// `(created, resolved)` deltas to fold into the summary.
async fn apply_alert(
    s: &AppState,
    page: StatusPageId,
    now: OffsetDateTime,
    alert: NormalizedAlert,
) -> Result<(usize, usize), ApiError> {
    match alert.action {
        AlertAction::Resolve => {
            if let Some(inc) =
                rampart_db::incidents::find_active_by_dedup_key(s.pool(), page, &alert.dedup_key)
                    .await?
            {
                rampart_db::incidents::resolve(s.pool(), inc.id, now).await?;
                return Ok((0, 1));
            }
            Ok((0, 0))
        }
        AlertAction::Create => {
            // A duplicate firing for an already-open incident would collide
            // with the partial unique index on (page, dedup_key) WHERE
            // active. Treat an existing active incident with this key as
            // already-reported and skip rather than erroring the webhook.
            if rampart_db::incidents::find_active_by_dedup_key(s.pool(), page, &alert.dedup_key)
                .await?
                .is_some()
            {
                return Ok((0, 0));
            }
            let new = NewIncident {
                title: alert.title,
                content: alert.content,
                style: alert.style,
                pinned: true,
                dedup_key: Some(alert.dedup_key),
            };
            rampart_db::incidents::create(s.pool(), page, None, new).await?;
            Ok((1, 0))
        }
    }
}

/// Drive a batch of normalized alerts through `apply_alert` and build the
/// HTTP response. Shared by the Alertmanager and Grafana handlers (both
/// deliver a list); Datadog delivers a single alert and calls it with a
/// one-element vec.
async fn ingest_batch(
    s: &AppState,
    page: StatusPageId,
    alerts: Vec<NormalizedAlert>,
) -> Result<(StatusCode, Json<IngestSummary>), ApiError> {
    let now = OffsetDateTime::now_utc();
    let mut created = 0usize;
    let mut resolved = 0usize;
    for alert in alerts {
        let (c, r) = apply_alert(s, page, now, alert).await?;
        created += c;
        resolved += r;
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

// ---- public: Alertmanager / Grafana (shared shape) ----------------------

/// Minimal shape of the Alertmanager / Grafana webhook payload. We only
/// read the fields we map onto incidents; everything else is ignored.
/// `#[serde(default)]` keeps us lenient about missing optional sections so
/// a partial/older payload still parses.
#[derive(Debug, Deserialize)]
struct PromAlertsPayload {
    #[serde(default)]
    alerts: Vec<PromAlert>,
}

#[derive(Debug, Deserialize)]
struct PromAlert {
    #[serde(default)]
    status: String,
    #[serde(default)]
    labels: std::collections::HashMap<String, String>,
    #[serde(default)]
    annotations: std::collections::HashMap<String, String>,
    /// Alertmanager (>=0.22) and Grafana unified alerting both stamp each
    /// alert with a stable `fingerprint`. We use it as the dedup key so
    /// resolve matches the exact firing incident; fall back to alertname
    /// when an older sender omits it.
    #[serde(default)]
    fingerprint: String,
}

/// Map one Prometheus-shaped alert (Alertmanager or Grafana) into the
/// normalized form. Returns `None` for an alert that carries no usable
/// title, so it is skipped rather than creating a blank incident.
fn normalize_prom_alert(alert: PromAlert) -> Option<NormalizedAlert> {
    let alertname = alert
        .labels
        .get("alertname")
        .map(String::as_str)
        .unwrap_or("")
        .trim();
    // Title: prefer the alertname, fall back to the summary annotation.
    // Skip alerts that carry neither.
    let title = if !alertname.is_empty() {
        alertname.to_string()
    } else {
        match alert.annotations.get("summary") {
            Some(sumtxt) if !sumtxt.trim().is_empty() => sumtxt.trim().to_string(),
            _ => return None,
        }
    };

    // Dedup key: prefer the vendor fingerprint (exact + stable across the
    // firing/resolved pair); fall back to the alertname so older senders
    // without a fingerprint still dedup, if loosely.
    let dedup_key = if !alert.fingerprint.trim().is_empty() {
        alert.fingerprint.trim().to_string()
    } else {
        title.clone()
    };

    let action = match alert.status.as_str() {
        "resolved" => AlertAction::Resolve,
        // Treat anything that isn't "resolved" as firing — both senders use
        // "firing", but a missing/unknown status shouldn't drop the alert.
        _ => AlertAction::Create,
    };

    let content = alert
        .annotations
        .get("description")
        .cloned()
        .or_else(|| alert.annotations.get("summary").cloned())
        .unwrap_or_default();
    let style = style_for_severity(alert.labels.get("severity").map(String::as_str));

    Some(NormalizedAlert {
        action,
        title,
        content,
        style,
        dedup_key,
    })
}

async fn alertmanager(
    State(s): State<AppState>,
    Path(token): Path<String>,
    Json(payload): Json<PromAlertsPayload>,
) -> Result<(StatusCode, Json<IngestSummary>), ApiError> {
    let page = page_for_token(&s, &token).await?;
    let alerts = payload.alerts.into_iter().filter_map(normalize_prom_alert);
    ingest_batch(&s, page, alerts.collect()).await
}

async fn grafana(
    State(s): State<AppState>,
    Path(token): Path<String>,
    Json(payload): Json<PromAlertsPayload>,
) -> Result<(StatusCode, Json<IngestSummary>), ApiError> {
    // Grafana's unified-alerting webhook body is the Alertmanager shape, so
    // the same parser applies verbatim.
    let page = page_for_token(&s, &token).await?;
    let alerts = payload.alerts.into_iter().filter_map(normalize_prom_alert);
    ingest_batch(&s, page, alerts.collect()).await
}

// ---- public: Datadog -----------------------------------------------------

/// Datadog's webhook body is operator-templated, but this matches the
/// documented default `$EVENT_*` template. We only read the fields we map.
#[derive(Debug, Deserialize)]
struct DatadogPayload {
    #[serde(default)]
    alert_type: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    alert_id: String,
    #[serde(default)]
    alert_transition: String,
}

fn datadog_style(alert_type: &str) -> IncidentStyle {
    match alert_type.to_ascii_lowercase().as_str() {
        "error" => IncidentStyle::Danger,
        "warning" => IncidentStyle::Warning,
        _ => IncidentStyle::Info,
    }
}

async fn datadog(
    State(s): State<AppState>,
    Path(token): Path<String>,
    Json(payload): Json<DatadogPayload>,
) -> Result<(StatusCode, Json<IngestSummary>), ApiError> {
    let page = page_for_token(&s, &token).await?;

    let title = payload.title.trim();
    // dedup_key = alert_id; fall back to the title so a payload missing the
    // id still dedups loosely. Skip an alert with neither.
    let dedup_key = if !payload.alert_id.trim().is_empty() {
        payload.alert_id.trim().to_string()
    } else if !title.is_empty() {
        title.to_string()
    } else {
        return Ok((
            StatusCode::ACCEPTED,
            Json(IngestSummary {
                created: 0,
                resolved: 0,
            }),
        ));
    };

    // A "Recovered" transition resolves; everything else ("Triggered",
    // re-trigger, etc.) creates — never silently drop.
    let action = if payload.alert_transition.eq_ignore_ascii_case("Recovered") {
        AlertAction::Resolve
    } else {
        AlertAction::Create
    };

    // Title is required for a create; if it's empty fall back to the
    // dedup_key so the incident is never titleless.
    let title = if title.is_empty() {
        dedup_key.clone()
    } else {
        title.to_string()
    };

    let alert = NormalizedAlert {
        action,
        title,
        content: payload.body,
        style: datadog_style(&payload.alert_type),
        dedup_key,
    };
    ingest_batch(&s, page, vec![alert]).await
}
