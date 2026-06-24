//! Inbound alert ingestion + ingest-token management.
//!
//! Three slices:
//!
//! - Public, token-authed webhook receivers:
//!   POST /v1/public/ingest/alertmanager/{token}  (Prometheus Alertmanager)
//!   POST /v1/public/ingest/grafana/{token}        (Grafana unified alerting)
//!   POST /v1/public/ingest/datadog/{token}        (Datadog webhook)
//!   POST /v1/public/ingest/pagerduty/{token}      (PagerDuty webhook v3)
//!   POST /v1/public/ingest/opsgenie/{token}       (Opsgenie webhook)
//!   POST /v1/public/ingest/generic/{token}        (operator-mapped JSON)
//!   Each accepts that vendor's webhook payload and turns the contained
//!   alert(s) into status-page incidents (firing → create, resolved →
//!   resolve the matching open incident). The token in the URL IS the auth
//!   — there is no session. They all normalize their payload into the same
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

use crate::auth::OrgContext;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use rampart_core::ids::{IngestTokenId, StatusPageId};
use rampart_core::ingest_token::{IngestMapping, IngestToken, NewIngestToken};
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
        .route("/ingest/pagerduty/{token}", post(pagerduty))
        .route("/ingest/opsgenie/{token}", post(opsgenie))
        .route("/ingest/generic/{token}", post(generic))
}

/// Page-scoped token management. Merged into the `/v1/status-pages` admin nest.
pub fn page_router() -> Router<AppState> {
    Router::new().route("/{id}/ingest-tokens", get(list_tokens).post(create_token))
}

/// Top-level revoke + mapping management. Nested at `/v1/ingest-tokens`.
pub fn token_router() -> Router<AppState> {
    Router::new()
        .route("/{id}", axum::routing::delete(revoke_token))
        .route("/{id}/mapping", axum::routing::patch(set_token_mapping))
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
    Extension(org): Extension<OrgContext>,
    Path(page): Path<String>,
) -> Result<Json<Vec<IngestToken>>, ApiError> {
    let page_id = parse_page(&page)?;
    // Org-gate via the owning page; a cross-org page id 404s before listing.
    s.store().get_status_page(page_id, org.org_id).await?;
    Ok(Json(s.store().list_ingest_tokens_for_page(page_id).await?))
}

async fn create_token(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Path(page): Path<String>,
    Json(input): Json<NewIngestToken>,
) -> Result<(StatusCode, Json<IngestToken>), ApiError> {
    let page_id = parse_page(&page)?;
    s.store().get_status_page(page_id, org.org_id).await?;
    let tok = s.store().create_ingest_token(page_id, input).await?;
    Ok((StatusCode::CREATED, Json(tok)))
}

async fn revoke_token(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    s.store()
        .delete_ingest_token(parse_token_id(&id)?, org.org_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Body for `PATCH /v1/ingest-tokens/{id}/mapping`. A `null` (or omitted)
/// `mapping` clears the configured mapping; an object sets it. The mapping is
/// validated by deserializing it into [`IngestMapping`] before it is stored,
/// so an unparseable shape is rejected with `400` rather than failing later at
/// ingest time.
#[derive(Debug, Deserialize)]
struct SetMappingBody {
    #[serde(default)]
    mapping: Option<serde_json::Value>,
}

async fn set_token_mapping(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Path(id): Path<String>,
    Json(body): Json<SetMappingBody>,
) -> Result<Json<IngestToken>, ApiError> {
    if let Some(m) = &body.mapping {
        serde_json::from_value::<IngestMapping>(m.clone())
            .map_err(|e| ApiError::BadRequest(format!("invalid mapping: {e}")))?;
    }
    let tok = s
        .store()
        .set_ingest_token_mapping(parse_token_id(&id)?, body.mapping, org.org_id)
        .await?;
    Ok(Json(tok))
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
    let tok = s
        .store()
        .find_ingest_token_by_token(token)
        .await
        .map_err(|_| ApiError::NotFound)?;
    // Best-effort last-used bump — don't fail the ingest on a touch error.
    let _ = s.store().touch_ingest_token_last_used(tok.id).await;
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
            if let Some(inc) = s
                .store()
                .find_active_incident_by_dedup_key(page, &alert.dedup_key)
                .await?
            {
                s.store().resolve_incident(inc.id, now).await?;
                crate::routes::status_pages::invalidate_public_view_cache();
                return Ok((0, 1));
            }
            Ok((0, 0))
        }
        AlertAction::Create => {
            // A duplicate firing for an already-open incident would collide
            // with the partial unique index on (page, dedup_key) WHERE
            // active. Treat an existing active incident with this key as
            // already-reported and skip rather than erroring the webhook.
            if s.store()
                .find_active_incident_by_dedup_key(page, &alert.dedup_key)
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
            // The pre-check above is a fast path, but two near-simultaneous
            // firings for the same dedup_key both pass it and both INSERT — the
            // second collides with the partial unique index
            // (status_page_id, dedup_key) WHERE active. Treat that unique
            // violation as already-reported (someone else just created it)
            // rather than erroring, so a concurrent duplicate doesn't 500 and
            // abort the rest of the batch.
            match s.store().create_incident(page, None, new).await {
                Ok(_) => {
                    crate::routes::status_pages::invalidate_public_view_cache();
                    Ok((1, 0))
                }
                Err(rampart_db::DbError::Sqlx(e))
                    if e.as_database_error()
                        .is_some_and(|d| d.is_unique_violation()) =>
                {
                    Ok((0, 0))
                }
                Err(e) => Err(e.into()),
            }
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

// ---- public: PagerDuty (Events API v2 / webhook v3) ----------------------

/// PagerDuty's v3 webhook envelope. We only read the fields we map; the
/// `event.data` block carries the incident, and `event.event_type` /
/// `data.status` tell us whether to create or resolve.
#[derive(Debug, Deserialize)]
struct PagerDutyPayload {
    #[serde(default)]
    event: PagerDutyEvent,
}

#[derive(Debug, Default, Deserialize)]
struct PagerDutyEvent {
    #[serde(default)]
    event_type: String,
    #[serde(default)]
    data: PagerDutyData,
}

#[derive(Debug, Default, Deserialize)]
struct PagerDutyData {
    #[serde(default)]
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    urgency: String,
    #[serde(default)]
    html_url: String,
}

fn pagerduty_style(urgency: &str) -> IncidentStyle {
    match urgency.to_ascii_lowercase().as_str() {
        "high" => IncidentStyle::Danger,
        _ => IncidentStyle::Warning,
    }
}

async fn pagerduty(
    State(s): State<AppState>,
    Path(token): Path<String>,
    Json(payload): Json<PagerDutyPayload>,
) -> Result<(StatusCode, Json<IngestSummary>), ApiError> {
    let page = page_for_token(&s, &token).await?;
    let data = payload.event.data;

    // dedup_key = incident id; fall back to title so a payload missing the
    // id still dedups loosely. Skip an event with neither.
    let title = data.title.trim();
    let dedup_key = if !data.id.trim().is_empty() {
        data.id.trim().to_string()
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

    // A resolved event (either `incident.resolved` or `status: "resolved"`)
    // resolves; `incident.acknowledged` and any other non-resolve action are
    // a no-op rather than opening a fresh incident, so an ack doesn't fan out
    // a duplicate. Only an explicit trigger creates.
    let event_type = data.status.trim();
    let is_resolved = payload
        .event
        .event_type
        .eq_ignore_ascii_case("incident.resolved")
        || event_type.eq_ignore_ascii_case("resolved");
    let is_triggered = payload
        .event
        .event_type
        .eq_ignore_ascii_case("incident.triggered")
        || event_type.eq_ignore_ascii_case("triggered");
    let action = if is_resolved {
        AlertAction::Resolve
    } else if is_triggered {
        AlertAction::Create
    } else {
        // e.g. incident.acknowledged — no-op.
        return Ok((
            StatusCode::ACCEPTED,
            Json(IngestSummary {
                created: 0,
                resolved: 0,
            }),
        ));
    };

    // Title is required for a create; fall back to the dedup_key so the
    // incident is never titleless.
    let title = if title.is_empty() {
        dedup_key.clone()
    } else {
        title.to_string()
    };

    let content = if data.html_url.trim().is_empty() {
        "PagerDuty incident".to_string()
    } else {
        format!("PagerDuty incident — {}", data.html_url.trim())
    };

    let alert = NormalizedAlert {
        action,
        title,
        content,
        style: pagerduty_style(&data.urgency),
        dedup_key,
    };
    ingest_batch(&s, page, vec![alert]).await
}

// ---- public: Opsgenie ----------------------------------------------------

/// Opsgenie's webhook body. The top-level `action` drives create/resolve;
/// the `alert` block carries the alert we map.
#[derive(Debug, Deserialize)]
struct OpsgeniePayload {
    #[serde(default)]
    action: String,
    #[serde(default)]
    alert: OpsgenieAlert,
}

#[derive(Debug, Default, Deserialize)]
struct OpsgenieAlert {
    #[serde(default, rename = "alertId")]
    alert_id: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    priority: String,
    #[serde(default, rename = "tinyId")]
    tiny_id: String,
}

fn opsgenie_style(priority: &str) -> IncidentStyle {
    match priority.to_ascii_uppercase().as_str() {
        "P1" | "P2" => IncidentStyle::Danger,
        "P3" => IncidentStyle::Warning,
        _ => IncidentStyle::Info,
    }
}

async fn opsgenie(
    State(s): State<AppState>,
    Path(token): Path<String>,
    Json(payload): Json<OpsgeniePayload>,
) -> Result<(StatusCode, Json<IngestSummary>), ApiError> {
    let page = page_for_token(&s, &token).await?;

    // Only Create / Close map to incident actions; AckAlert and the rest are
    // a deliberate no-op (202 with created:0 resolved:0).
    let action = if payload.action.eq_ignore_ascii_case("Create") {
        AlertAction::Create
    } else if payload.action.eq_ignore_ascii_case("Close") {
        AlertAction::Resolve
    } else {
        return Ok((
            StatusCode::ACCEPTED,
            Json(IngestSummary {
                created: 0,
                resolved: 0,
            }),
        ));
    };

    let alert = payload.alert;
    let message = alert.message.trim();
    // dedup_key = alertId; fall back to tinyId then message so a payload
    // missing the id still dedups loosely. Skip an alert with none.
    let dedup_key = if !alert.alert_id.trim().is_empty() {
        alert.alert_id.trim().to_string()
    } else if !alert.tiny_id.trim().is_empty() {
        alert.tiny_id.trim().to_string()
    } else if !message.is_empty() {
        message.to_string()
    } else {
        return Ok((
            StatusCode::ACCEPTED,
            Json(IngestSummary {
                created: 0,
                resolved: 0,
            }),
        ));
    };

    // Title is required for a create; fall back to the dedup_key so the
    // incident is never titleless.
    let title = if message.is_empty() {
        dedup_key.clone()
    } else {
        message.to_string()
    };

    let alert = NormalizedAlert {
        action,
        title,
        content: alert.description,
        style: opsgenie_style(&alert.priority),
        dedup_key,
    };
    ingest_batch(&s, page, vec![alert]).await
}

// ---- public: generic JSON-path receiver ----------------------------------

/// Parse a mapping's `style` string into an `IncidentStyle`, defaulting to
/// `Info` when absent or unrecognized.
fn style_from_mapping(style: Option<&str>) -> IncidentStyle {
    match style.map(str::to_ascii_lowercase).as_deref() {
        Some("warning") => IncidentStyle::Warning,
        Some("danger") => IncidentStyle::Danger,
        Some("primary") => IncidentStyle::Primary,
        Some("success") => IncidentStyle::Success,
        _ => IncidentStyle::Info,
    }
}

/// Read the value at an optional RFC 6901 JSON Pointer as a trimmed string.
/// Both JSON strings and scalars (numbers / bools) are coerced to text so a
/// `"status": "firing"` and a `"firing": true`-style discriminator both work.
/// Returns `None` for a missing pointer, an absent path, or a null/empty
/// value.
fn pointer_str(body: &serde_json::Value, pointer: Option<&str>) -> Option<String> {
    let ptr = pointer?;
    let v = body.pointer(ptr)?;
    let s = match v {
        serde_json::Value::String(s) => s.trim().to_string(),
        serde_json::Value::Null => return None,
        other => other.to_string(),
    };
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Generic operator-mapped receiver. The token's stored `mapping` describes
/// how to pull the normalized incident fields out of an arbitrary inbound
/// body via JSON Pointers, so any tool that can POST JSON works without a
/// dedicated vendor parser.
async fn generic(
    State(s): State<AppState>,
    Path(token): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<(StatusCode, Json<IngestSummary>), ApiError> {
    // Resolve the token to its record (404 on unknown) and require a mapping.
    let tok = s
        .store()
        .find_ingest_token_by_token(&token)
        .await
        .map_err(|_| ApiError::NotFound)?;
    let _ = s.store().touch_ingest_token_last_used(tok.id).await;
    let page = tok.status_page_id;

    let raw = tok.mapping.ok_or_else(|| {
        ApiError::BadRequest(
            "this ingest token has no generic mapping configured; \
             set one via PATCH /v1/ingest-tokens/{id}/mapping"
                .into(),
        )
    })?;
    let mapping: IngestMapping = serde_json::from_value(raw)
        .map_err(|e| ApiError::BadRequest(format!("stored mapping is invalid: {e}")))?;

    // Action: compare the value at action_path against the configured
    // firing/resolved values. An unrecognized value (or absent path/pointer)
    // falls through to Create so a present alert is never silently dropped.
    let action_val = pointer_str(&body, mapping.action_path.as_deref());
    let action = match action_val.as_deref() {
        Some(v) if mapping.action_resolved_value.as_deref() == Some(v) => AlertAction::Resolve,
        Some(v) if mapping.action_firing_value.as_deref() == Some(v) => AlertAction::Create,
        _ => AlertAction::Create,
    };

    let content = pointer_str(&body, mapping.content_path.as_deref()).unwrap_or_default();

    // Dedup key: the value at dedup_path; fall back to the title so a body
    // without a stable key still dedups loosely. If neither is present the
    // alert is unidentifiable, so it's a no-op rather than a blank incident.
    let title_val = pointer_str(&body, mapping.title_path.as_deref());
    let dedup_key = match pointer_str(&body, mapping.dedup_path.as_deref()).or_else(|| {
        title_val
            .clone()
            .or_else(|| (!content.is_empty()).then(|| content.clone()))
    }) {
        Some(k) => k,
        None => {
            return Ok((
                StatusCode::ACCEPTED,
                Json(IngestSummary {
                    created: 0,
                    resolved: 0,
                }),
            ));
        }
    };

    // Title is required for a create; fall back to the dedup key so the
    // incident is never titleless.
    let title = title_val.unwrap_or_else(|| dedup_key.clone());

    let alert = NormalizedAlert {
        action,
        title,
        content,
        style: style_from_mapping(mapping.style.as_deref()),
        dedup_key,
    };
    ingest_batch(&s, page, vec![alert]).await
}
