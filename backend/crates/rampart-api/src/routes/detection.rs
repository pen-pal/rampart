//! Detection rule + finding API (`/v1/detection-rules`).
//!
//! SIEM-style rules over the log tier. The scheduler evaluates them on its
//! slow tick (`rampart_db::detection`); these routes manage rules and triage
//! the findings they raise. Mounted in the editor slice — editors (incl. a SOC
//! role) manage rules and acknowledge findings; readonly users GET.

use crate::auth::OrgContext;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Extension, Path, Query, State};
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

async fn list(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
) -> Result<Json<Vec<DetectionRule>>, ApiError> {
    Ok(Json(rampart_db::detection::list(s.pool(), org.org_id).await?))
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
    validate_condition(&s, input.condition.as_ref()).await?;
    let rule = rampart_db::detection::create(s.pool(), input).await?;
    Ok((StatusCode::CREATED, Json(rule)))
}

async fn update(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
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
    validate_condition(&s, input.condition.as_ref()).await?;
    Ok(Json(
        rampart_db::detection::update(s.pool(), rule_id, input, org.org_id).await?,
    ))
}

/// Validate a Detection v2 boolean condition tree before it's stored: bounded
/// size/depth + non-empty leaves (structural), plus a real regex check on every
/// `BodyRegex` leaf so a bad pattern can't break the evaluation tick.
async fn validate_condition(
    s: &AppState,
    cond: Option<&rampart_core::DetectionCondition>,
) -> Result<(), ApiError> {
    let Some(cond) = cond else { return Ok(()) };
    cond.validate_tree().map_err(ApiError::BadRequest)?;
    let mut regexes = Vec::new();
    cond.body_regexes(&mut regexes);
    for rx in regexes {
        if !rampart_db::detection::regex_is_valid(s.pool(), rx).await? {
            return Err(ApiError::BadRequest(format!(
                "condition body_regex is not a valid regex: {rx}"
            )));
        }
    }
    Ok(())
}

async fn delete_rule(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    rampart_db::detection::delete(s.pool(), parse_rule_id(&id)?, org.org_id).await?;
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
    Extension(org): Extension<OrgContext>,
    Query(q): Query<FindingsQuery>,
) -> Result<Json<Vec<DetectionFinding>>, ApiError> {
    let limit = q.limit.unwrap_or(100).clamp(1, 1000);
    Ok(Json(
        rampart_db::detection::list_findings_for_org(s.pool(), limit, q.open, org.org_id).await?,
    ))
}

async fn ack_finding(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Path(id): Path<String>,
) -> Result<Json<DetectionFinding>, ApiError> {
    let fid = parse_finding_id(&id)?;
    // Gate through the finding's owning rule's org — cross-org finding = 404.
    rampart_db::detection::finding_in_org(s.pool(), fid, org.org_id).await?;
    Ok(Json(rampart_db::detection::ack_finding(s.pool(), fid).await?))
}

#[derive(Deserialize)]
struct PreviewBody {
    #[serde(default)]
    service: String,
    #[serde(default)]
    min_level: i16,
    #[serde(default)]
    body_regex: String,
    #[serde(default)]
    attr_key: String,
    #[serde(default)]
    attr_val: String,
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
        rampart_db::detection::preview(
            s.pool(),
            &b.service,
            min_level,
            &b.body_regex,
            &b.attr_key,
            &b.attr_val,
            window,
        )
        .await?,
    ))
}
