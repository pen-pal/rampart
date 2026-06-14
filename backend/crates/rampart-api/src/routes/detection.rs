//! Detection rule + finding API (`/v1/detection-rules`).
//!
//! SIEM-style rules over the log tier. The scheduler evaluates them on its
//! slow tick (`rampart_db::detection`); these routes manage rules and triage
//! the findings they raise. Mounted in the editor slice — editors (incl. a SOC
//! role) manage rules and acknowledge findings; readonly users GET.

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use rampart_core::detection::{
    DetectionFinding, DetectionRule, NewDetectionRule, UpdateDetectionRule,
};
use rampart_core::ids::{DetectionFindingId, DetectionRuleId};
use serde::Deserialize;
use std::str::FromStr;
use uuid::Uuid;
use validator::Validate;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        // static `/findings*` segments resolve before the `/{id}` param in
        // axum 0.8, so rule and finding routes coexist on one mount.
        .route("/findings", get(list_findings))
        .route("/findings/{id}/ack", post(ack_finding))
        .route("/preview", post(preview))
        .route("/{id}", axum::routing::patch(update).delete(delete_rule))
}

fn parse_rule_id(s: &str) -> Result<DetectionRuleId, ApiError> {
    Uuid::from_str(s)
        .map(DetectionRuleId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid rule id".into()))
}

fn parse_finding_id(s: &str) -> Result<DetectionFindingId, ApiError> {
    Uuid::from_str(s)
        .map(DetectionFindingId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid finding id".into()))
}

async fn list(State(s): State<AppState>) -> Result<Json<Vec<DetectionRule>>, ApiError> {
    Ok(Json(rampart_db::detection::list(s.pool()).await?))
}

async fn create(
    State(s): State<AppState>,
    Json(input): Json<NewDetectionRule>,
) -> Result<(StatusCode, Json<DetectionRule>), ApiError> {
    input
        .validate()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    if !rampart_db::detection::regex_is_valid(s.pool(), &input.body_regex).await? {
        return Err(ApiError::BadRequest("body_regex is not a valid regex".into()));
    }
    let rule = rampart_db::detection::create(s.pool(), input).await?;
    Ok((StatusCode::CREATED, Json(rule)))
}

async fn update(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<UpdateDetectionRule>,
) -> Result<Json<DetectionRule>, ApiError> {
    let rule_id = parse_rule_id(&id)?;
    input
        .validate()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    if let Some(rx) = input.body_regex.as_deref() {
        if !rampart_db::detection::regex_is_valid(s.pool(), rx).await? {
            return Err(ApiError::BadRequest("body_regex is not a valid regex".into()));
        }
    }
    Ok(Json(
        rampart_db::detection::update(s.pool(), rule_id, input).await?,
    ))
}

async fn delete_rule(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    rampart_db::detection::delete(s.pool(), parse_rule_id(&id)?).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct FindingsQuery {
    #[serde(default)]
    open: bool,
    limit: Option<i64>,
}

async fn list_findings(
    State(s): State<AppState>,
    Query(q): Query<FindingsQuery>,
) -> Result<Json<Vec<DetectionFinding>>, ApiError> {
    let limit = q.limit.unwrap_or(100).clamp(1, 1000);
    Ok(Json(
        rampart_db::detection::list_findings(s.pool(), limit, q.open).await?,
    ))
}

async fn ack_finding(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<DetectionFinding>, ApiError> {
    Ok(Json(
        rampart_db::detection::ack_finding(s.pool(), parse_finding_id(&id)?).await?,
    ))
}

#[derive(Deserialize)]
struct PreviewBody {
    #[serde(default)]
    service: String,
    #[serde(default)]
    min_level: i16,
    #[serde(default)]
    body_regex: String,
    #[serde(default = "default_preview_window")]
    window_seconds: i32,
}

fn default_preview_window() -> i32 {
    300
}

/// Dry-run a rule spec over recent logs without saving — the "test rule" path.
async fn preview(
    State(s): State<AppState>,
    Json(b): Json<PreviewBody>,
) -> Result<Json<rampart_db::detection::PreviewResult>, ApiError> {
    if !rampart_db::detection::regex_is_valid(s.pool(), &b.body_regex).await? {
        return Err(ApiError::BadRequest("body_regex is not a valid regex".into()));
    }
    let window = b.window_seconds.clamp(1, 86_400);
    let min_level = b.min_level.clamp(0, 24);
    Ok(Json(
        rampart_db::detection::preview(s.pool(), &b.service, min_level, &b.body_regex, window)
            .await?,
    ))
}
