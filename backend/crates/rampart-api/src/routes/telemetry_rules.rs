//! Telemetry alert rule CRUD (`/v1/telemetry-rules`).
//!
//! Threshold rules over the error / trace / log tiers. The scheduler
//! evaluates them on its slow tick (`rampart_db::telemetry_rules`); these
//! routes are just management. Mounted in the editor slice — editors manage
//! alerting, readonly users GET.

use crate::auth::OrgContext;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use rampart_core::ids::TelemetryRuleId;
use rampart_core::telemetry_rule::{NewTelemetryRule, TelemetryRule, UpdateTelemetryRule};
use std::str::FromStr;
use uuid::Uuid;
use validator::Validate;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", axum::routing::patch(update).delete(delete_rule))
}

fn parse_id(s: &str) -> Result<TelemetryRuleId, ApiError> {
    Uuid::from_str(s)
        .map(TelemetryRuleId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid rule id".into()))
}

async fn list(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
) -> Result<Json<Vec<TelemetryRule>>, ApiError> {
    Ok(Json(s.store().list_telemetry_rules(org.org_id).await?))
}

async fn create(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Json(input): Json<NewTelemetryRule>,
) -> Result<(StatusCode, Json<TelemetryRule>), ApiError> {
    input
        .validate()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let rule = s.store().create_telemetry_rule(input, org.org_id).await?;
    Ok((StatusCode::CREATED, Json(rule)))
}

async fn update(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Path(id): Path<String>,
    Json(input): Json<UpdateTelemetryRule>,
) -> Result<Json<TelemetryRule>, ApiError> {
    let rule_id = parse_id(&id)?;
    input
        .validate()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(
        s.store()
            .update_telemetry_rule(rule_id, input, org.org_id)
            .await?,
    ))
}

async fn delete_rule(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    s.store()
        .delete_telemetry_rule(parse_id(&id)?, org.org_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
