//! Status-page subscribers and SMTP settings.
//!
//! Subscribe + unsubscribe are public (the whole point is anonymous
//! interest in a page). List + delete sit on the admin side. SMTP
//! settings are admin-only.

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum::{Json, Router};
use rampart_core::ids::{StatusPageId, StatusPageSubscriberId};
use rampart_db::subscribers::Subscriber;
use serde::Deserialize;
use std::str::FromStr;
use uuid::Uuid;

pub fn public_router() -> Router<AppState> {
    Router::new()
        .route("/status-pages/{slug}/subscribe", post(subscribe))
        .route("/subscribe/unsubscribe/{token}", get(unsubscribe))
        .route("/subscribers/manage/{token}", get(manage))
        .route(
            "/subscribers/manage/{token}/unsubscribe-all",
            post(unsubscribe_all),
        )
        .route(
            "/subscribers/manage/{token}/unsubscribe/{slug}",
            post(unsubscribe_page),
        )
}

/// Status-page subscriber management — editors may manage these (they're
/// part of running a status page), so this router carries no admin gate.
/// The readonly method-guard on the protected tree still blocks readonly
/// users from the mutating verbs here.
pub fn admin_router() -> Router<AppState> {
    Router::new()
        .route("/status-pages/{id}/subscribers", get(list))
        .route("/subscribers/{id}", axum::routing::delete(delete_one))
}

/// SMTP + retention settings — admin-only. Mounted separately so the
/// caller can layer `require_admin` over just these.
pub fn settings_router() -> Router<AppState> {
    Router::new()
        .route("/settings/smtp", get(smtp_get).put(smtp_put))
        .route("/settings/retention", get(retention_get).put(retention_put))
        .route(
            "/settings/telemetry-token",
            get(telemetry_token_get).put(telemetry_token_put),
        )
}

#[derive(Deserialize)]
struct SubscribeInput {
    email: String,
}

async fn subscribe(
    State(s): State<AppState>,
    Path(slug): Path<String>,
    Json(input): Json<SubscribeInput>,
) -> Result<StatusCode, ApiError> {
    if !input.email.contains('@') {
        return Err(ApiError::BadRequest("email looks invalid".into()));
    }
    let page = rampart_db::status_pages::get_by_slug(s.pool(), &slug).await?;
    rampart_db::subscribers::subscribe_email(s.pool(), page.id, &input.email).await?;
    // No confirmation email in v1 (single-opt-in). Future: send a
    // confirm link instead and only flip `confirmed` on click.
    Ok(StatusCode::ACCEPTED)
}

async fn unsubscribe(
    State(s): State<AppState>,
    Path(token): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    // Idempotent: missing token still returns the same friendly page.
    let _ = rampart_db::subscribers::unsubscribe_by_token(s.pool(), &token).await;
    // Tiny self-contained page — no SPA, no auth, just a confirmation.
    Ok(Html(
        r#"<!doctype html><html><head><meta charset="utf-8">
        <title>Unsubscribed</title>
        <style>body{font-family:system-ui;max-width:520px;margin:80px auto;padding:0 24px;color:#1c1917}h1{font-size:22px;margin:0 0 6px}p{color:#57534e}</style>
        </head><body><h1>Unsubscribed.</h1>
        <p>You won't receive further updates from this status page.</p></body></html>"#,
    ))
}

// ── Self-manage (token-based, no auth) ───────────────────────────────────

#[derive(serde::Serialize)]
struct ManageView {
    email: String,
    subscriptions: Vec<rampart_db::subscribers::ManagedSubscription>,
}

/// Resolve a subscriber by any of their per-page unsubscribe tokens and
/// return the email plus every page that email is subscribed to.
async fn manage(
    State(s): State<AppState>,
    Path(token): Path<String>,
) -> Result<Json<ManageView>, ApiError> {
    let email = rampart_db::subscribers::email_for_token(s.pool(), &token)
        .await?
        .ok_or(ApiError::NotFound)?;
    let subscriptions = rampart_db::subscribers::subscriptions_for_email(s.pool(), &email).await?;
    Ok(Json(ManageView {
        email,
        subscriptions,
    }))
}

#[derive(serde::Serialize)]
struct RemovedCount {
    removed: u64,
}

/// Remove the token's email from every page in one click.
async fn unsubscribe_all(
    State(s): State<AppState>,
    Path(token): Path<String>,
) -> Result<Json<RemovedCount>, ApiError> {
    let email = rampart_db::subscribers::email_for_token(s.pool(), &token)
        .await?
        .ok_or(ApiError::NotFound)?;
    let removed = rampart_db::subscribers::unsubscribe_all_for_email(s.pool(), &email).await?;
    Ok(Json(RemovedCount { removed }))
}

/// Remove the token's email from a single page (by slug). Scoped to the
/// token's email so one subscriber can't unsubscribe another.
async fn unsubscribe_page(
    State(s): State<AppState>,
    Path((token, slug)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let email = rampart_db::subscribers::email_for_token(s.pool(), &token)
        .await?
        .ok_or(ApiError::NotFound)?;
    let page = rampart_db::status_pages::get_by_slug(s.pool(), &slug).await?;
    rampart_db::subscribers::unsubscribe_email_from_page(s.pool(), page.id, &email).await?;
    Ok(StatusCode::NO_CONTENT)
}

fn parse_page(s: &str) -> Result<StatusPageId, ApiError> {
    Uuid::from_str(s)
        .map(StatusPageId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid status page id".into()))
}
fn parse_sub(s: &str) -> Result<StatusPageSubscriberId, ApiError> {
    Uuid::from_str(s)
        .map(StatusPageSubscriberId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid subscriber id".into()))
}

async fn list(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<Subscriber>>, ApiError> {
    Ok(Json(
        rampart_db::subscribers::list_for_page(s.pool(), parse_page(&id)?).await?,
    ))
}

async fn delete_one(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    rampart_db::subscribers::delete(s.pool(), parse_sub(&id)?).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── SMTP settings ────────────────────────────────────────────────────────

async fn smtp_get(State(s): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    let raw = rampart_db::settings::get(s.pool(), "smtp").await?;
    // Redact password if present — the admin can re-enter when editing.
    let value = raw.map(|mut v| {
        if let Some(obj) = v.as_object_mut() {
            if obj.contains_key("password") {
                obj.insert(
                    "password".into(),
                    serde_json::Value::String("(redacted)".into()),
                );
            }
        }
        v
    });
    Ok(Json(value.unwrap_or(serde_json::Value::Null)))
}

#[derive(Deserialize)]
struct SmtpPut {
    #[serde(flatten)]
    cfg: serde_json::Value,
}

async fn smtp_put(
    State(s): State<AppState>,
    Json(input): Json<SmtpPut>,
) -> Result<StatusCode, ApiError> {
    // Validate by deserialising into the typed shape.
    let parsed: crate::smtp::SmtpConfig = serde_json::from_value(input.cfg.clone())
        .map_err(|e| ApiError::BadRequest(format!("smtp config: {e}")))?;
    // If the caller sent the literal "(redacted)" placeholder, keep
    // the old password instead of overwriting.
    let mut new_value = input.cfg;
    if parsed.password.as_deref() == Some("(redacted)") {
        if let Some(existing) = rampart_db::settings::get(s.pool(), "smtp").await? {
            if let (Some(new_obj), Some(old_obj)) =
                (new_value.as_object_mut(), existing.as_object())
            {
                if let Some(old_pw) = old_obj.get("password") {
                    new_obj.insert("password".into(), old_pw.clone());
                }
            }
        }
    }
    rampart_db::settings::put(s.pool(), "smtp", &new_value).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Retention settings ────────────────────────────────────────────────────

async fn retention_get(State(s): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    // Default mirrors `RetentionConfig::default()` in rampart-db::prune.
    let raw = rampart_db::settings::get(s.pool(), "retention_days").await?;
    Ok(Json(raw.unwrap_or_else(
        || serde_json::json!({ "heartbeats": 90, "audit_log": 365 }),
    )))
}

#[derive(Deserialize)]
struct RetentionPut {
    heartbeats: i32,
    audit_log: i32,
}

async fn retention_put(
    State(s): State<AppState>,
    Json(input): Json<RetentionPut>,
) -> Result<StatusCode, ApiError> {
    if input.heartbeats < 1 || input.audit_log < 1 {
        return Err(ApiError::BadRequest(
            "retention windows must be >= 1 day".into(),
        ));
    }
    let value = serde_json::json!({
        "heartbeats": input.heartbeats,
        "audit_log":  input.audit_log,
    });
    rampart_db::settings::put(s.pool(), "retention_days", &value).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Telemetry ingest token ────────────────────────────────────────────────
// Optional shared secret guarding the root-level OTLP + RUM ingest surfaces.
// Empty/absent => those endpoints stay open (the operator controls network
// exposure). Admin-only; the value is returned in the clear so the operator
// can paste it into their collector/exporter config (like an ingest token).

async fn telemetry_token_get(
    State(s): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let token = crate::ingest_util::configured_token(s.pool())
        .await?
        .unwrap_or_default();
    Ok(Json(serde_json::json!({ "token": token })))
}

#[derive(Deserialize)]
struct TelemetryTokenPut {
    token: String,
}

async fn telemetry_token_put(
    State(s): State<AppState>,
    Json(input): Json<TelemetryTokenPut>,
) -> Result<StatusCode, ApiError> {
    let trimmed = input.token.trim();
    if trimmed.is_empty() {
        // Clear => disable auth (re-open the ingest surfaces).
        rampart_db::settings::delete(s.pool(), crate::ingest_util::TELEMETRY_TOKEN_KEY).await?;
    } else {
        rampart_db::settings::put(
            s.pool(),
            crate::ingest_util::TELEMETRY_TOKEN_KEY,
            &serde_json::Value::String(trimmed.to_owned()),
        )
        .await?;
    }
    Ok(StatusCode::NO_CONTENT)
}
