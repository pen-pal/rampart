//! Status-page subscribers and SMTP settings.
//!
//! Subscribe + unsubscribe are public (the whole point is anonymous
//! interest in a page). List + delete sit on the admin side. SMTP
//! settings are admin-only.

use crate::auth::OrgContext;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum::{Json, Router};
use rampart_core::ids::{StatusPageId, StatusPageSubscriberId};
use rampart_db::subscribers::Subscriber;
use serde::Deserialize;
use std::str::FromStr;
use uuid::Uuid;

pub fn public_router(limiter: crate::rate_limit::IpRateLimiter) -> Router<AppState> {
    // `/subscribe` is unauthenticated and the attacker controls the email, so the
    // per-(page,email) ON CONFLICT dedup doesn't stop a flood of distinct
    // addresses growing the table / bloating notification fan-out. Throttle it
    // per-IP (same limiter as /auth — a real subscriber signs up once). The
    // token-scoped unsubscribe/manage routes below carry their own opaque token
    // and aren't a growth vector, so they stay unthrottled.
    let subscribe = Router::new()
        .route("/status-pages/{slug}/subscribe", post(subscribe))
        .route_layer(axum::middleware::from_fn_with_state(
            limiter,
            crate::rate_limit::enforce_ip_rate_limit,
        ));
    Router::new()
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
        .merge(subscribe)
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
        .route("/settings/siem-export", get(siem_get).put(siem_put))
        .route("/settings/security", get(security_get).put(security_put))
        .route("/settings/storage", get(storage_get))
        .route(
            "/settings/ingest-sampling",
            get(sampling_get).put(sampling_put),
        )
}

/// Head-sampling keep-percentages for traces + logs (admin). 100 = off.
async fn sampling_get(
    State(s): State<AppState>,
) -> Result<Json<crate::ingest_util::SamplingConfig>, ApiError> {
    Ok(Json(crate::ingest_util::sampling_config(s.pool()).await?))
}

async fn sampling_put(
    State(s): State<AppState>,
    Json(input): Json<crate::ingest_util::SamplingConfig>,
) -> Result<StatusCode, ApiError> {
    if [input.traces_pct, input.logs_pct]
        .iter()
        .any(|p| !(0..=100).contains(p))
    {
        return Err(ApiError::BadRequest(
            "sampling percent must be 0-100".into(),
        ));
    }
    let value = serde_json::to_value(&input)
        .map_err(|e| ApiError::BadRequest(format!("invalid sampling config: {e}")))?;
    s.store()
        .put_setting(crate::ingest_util::SAMPLING_KEY, &value)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Per-tier on-disk usage (admin) — the data-footprint panel. Read-only.
async fn storage_get(
    State(s): State<AppState>,
) -> Result<Json<Vec<rampart_db::metrics::TableSize>>, ApiError> {
    Ok(Json(s.store().storage_usage().await?))
}

async fn security_get(State(s): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    let raw = s.store().get_setting("require_2fa").await?;
    let require_2fa = raw
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "off".to_string());
    Ok(Json(serde_json::json!({ "require_2fa": require_2fa })))
}

#[derive(Deserialize)]
struct SecurityPut {
    require_2fa: String,
}

async fn security_put(
    State(s): State<AppState>,
    Json(input): Json<SecurityPut>,
) -> Result<StatusCode, ApiError> {
    if !matches!(input.require_2fa.as_str(), "off" | "admins" | "all") {
        return Err(ApiError::BadRequest(
            "require_2fa must be one of: off, admins, all".into(),
        ));
    }
    s.store()
        .put_setting("require_2fa", &serde_json::json!(input.require_2fa))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn siem_get(State(s): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    let raw = s.store().get_setting("siem_export").await?;
    Ok(Json(raw.unwrap_or_else(|| {
        serde_json::json!({ "enabled": false, "kind": "webhook", "target": "", "format": "json" })
    })))
}

#[derive(Deserialize)]
struct SiemPut {
    enabled: bool,
    kind: String,
    target: String,
    /// "json" (default), "cef", or "leef". Absent/empty is treated as "json".
    #[serde(default)]
    format: String,
}

async fn siem_put(
    State(s): State<AppState>,
    Json(input): Json<SiemPut>,
) -> Result<StatusCode, ApiError> {
    let format = if input.format.trim().is_empty() {
        "json"
    } else {
        input.format.trim()
    };
    if !matches!(format, "json" | "cef" | "leef") {
        return Err(ApiError::BadRequest(
            "format must be json, cef, or leef".into(),
        ));
    }
    if input.enabled {
        if !matches!(input.kind.as_str(), "webhook" | "syslog" | "syslog_tcp") {
            return Err(ApiError::BadRequest(
                "kind must be webhook, syslog, or syslog_tcp".into(),
            ));
        }
        if input.target.trim().is_empty() {
            return Err(ApiError::BadRequest(
                "target is required when export is enabled".into(),
            ));
        }
    }
    let value = serde_json::json!({
        "enabled": input.enabled,
        "kind": input.kind,
        "target": input.target.trim(),
        "format": format,
    });
    s.store().put_setting("siem_export", &value).await?;
    Ok(StatusCode::NO_CONTENT)
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
    let page = s.store().get_status_page_by_slug(&slug).await?;
    s.store().subscribe_email(page.id, &input.email).await?;
    // No confirmation email in v1 (single-opt-in). Future: send a
    // confirm link instead and only flip `confirmed` on click.
    Ok(StatusCode::ACCEPTED)
}

async fn unsubscribe(
    State(s): State<AppState>,
    Path(token): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    // Idempotent: missing token still returns the same friendly page.
    let _ = s.store().unsubscribe_subscriber_by_token(&token).await;
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
    let email = s
        .store()
        .subscriber_email_for_token(&token)
        .await?
        .ok_or(ApiError::NotFound)?;
    let subscriptions = s.store().subscriptions_for_email(&email).await?;
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
    let email = s
        .store()
        .subscriber_email_for_token(&token)
        .await?
        .ok_or(ApiError::NotFound)?;
    let removed = s.store().unsubscribe_all_for_email(&email).await?;
    Ok(Json(RemovedCount { removed }))
}

/// Remove the token's email from a single page (by slug). Scoped to the
/// token's email so one subscriber can't unsubscribe another.
async fn unsubscribe_page(
    State(s): State<AppState>,
    Path((token, slug)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let email = s
        .store()
        .subscriber_email_for_token(&token)
        .await?
        .ok_or(ApiError::NotFound)?;
    let page = s.store().get_status_page_by_slug(&slug).await?;
    s.store()
        .unsubscribe_email_from_page(page.id, &email)
        .await?;
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
    Extension(org): Extension<OrgContext>,
    Path(id): Path<String>,
) -> Result<Json<Vec<Subscriber>>, ApiError> {
    let page_id = parse_page(&id)?;
    // Org-gate via the parent page: cross-org pages 404 here, so the
    // subscriber list never leaks across tenants.
    s.store().get_status_page(page_id, org.org_id).await?;
    Ok(Json(s.store().list_subscribers_for_page(page_id).await?))
}

async fn delete_one(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let sub_id = parse_sub(&id)?;
    // Resolve the subscriber's parent page, then org-gate it: a missing
    // subscriber or one on another org's page both 404 (no leak).
    let page_id = s
        .store()
        .subscriber_page_for(sub_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    s.store().get_status_page(page_id, org.org_id).await?;
    s.store().delete_subscriber(sub_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── SMTP settings ────────────────────────────────────────────────────────

async fn smtp_get(State(s): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    let raw = s.store().get_setting("smtp").await?;
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
        if let Some(existing) = s.store().get_setting("smtp").await? {
            if let (Some(new_obj), Some(old_obj)) =
                (new_value.as_object_mut(), existing.as_object())
            {
                if let Some(old_pw) = old_obj.get("password") {
                    new_obj.insert("password".into(), old_pw.clone());
                }
            }
        }
    }
    s.store().put_setting("smtp", &new_value).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Retention settings ────────────────────────────────────────────────────

async fn retention_get(State(s): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    // Return the *effective* config: the stored blob with every missing tier
    // filled by its real default (so the UI shows exactly what the prune loop
    // uses, not a partial blob).
    let cfg = rampart_db::prune::load_config(s.pool()).await?;
    Ok(Json(
        serde_json::to_value(cfg).unwrap_or_else(|_| serde_json::json!({})),
    ))
}

async fn retention_put(
    State(s): State<AppState>,
    // Accept the full config; missing fields fall back to their defaults via
    // RetentionConfig's serde defaults, so older clients still work.
    Json(input): Json<rampart_db::prune::RetentionConfig>,
) -> Result<StatusCode, ApiError> {
    let windows = [
        input.heartbeats,
        input.audit_log,
        input.rollup_days,
        input.metrics_days,
        input.traces_days,
        input.logs_days,
        input.rum_days,
        input.profiles_days,
    ];
    if windows.iter().any(|d| !(1..=36500).contains(d)) {
        return Err(ApiError::BadRequest(
            "retention windows must be between 1 and 36500 days".into(),
        ));
    }
    let value = serde_json::to_value(&input)
        .map_err(|e| ApiError::BadRequest(format!("invalid retention config: {e}")))?;
    s.store().put_setting("retention_days", &value).await?;
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
        s.store()
            .delete_setting(crate::ingest_util::TELEMETRY_TOKEN_KEY)
            .await?;
    } else {
        s.store()
            .put_setting(
                crate::ingest_util::TELEMETRY_TOKEN_KEY,
                &serde_json::Value::String(trimmed.to_owned()),
            )
            .await?;
    }
    Ok(StatusCode::NO_CONTENT)
}
