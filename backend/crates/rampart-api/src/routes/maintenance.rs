//! `/v1/maintenance-windows` routes.

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use rampart_core::ids::{MaintenanceId, MonitorId};
use rampart_core::maintenance::{MaintenanceWindow, NewMaintenanceWindow};
use std::str::FromStr;
use uuid::Uuid;
use validator::Validate;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/:id", get(get_one).delete(remove))
        .route("/:id/active", post(set_active))
        .route("/:id/monitors/:monitor_id", post(attach).delete(detach))
}

fn parse_id(s: &str) -> Result<MaintenanceId, ApiError> {
    Uuid::from_str(s)
        .map(MaintenanceId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid window id".into()))
}
fn parse_monitor(s: &str) -> Result<MonitorId, ApiError> {
    Uuid::from_str(s)
        .map(MonitorId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid monitor id".into()))
}

async fn list(State(s): State<AppState>) -> Result<Json<Vec<MaintenanceWindow>>, ApiError> {
    Ok(Json(rampart_db::maintenance::list(s.pool()).await?))
}

async fn get_one(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<MaintenanceWindow>, ApiError> {
    Ok(Json(
        rampart_db::maintenance::get(s.pool(), parse_id(&id)?).await?,
    ))
}

async fn create(
    State(s): State<AppState>,
    Json(input): Json<NewMaintenanceWindow>,
) -> Result<(StatusCode, Json<MaintenanceWindow>), ApiError> {
    input
        .validate()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    if input.end_at <= input.start_at {
        return Err(ApiError::BadRequest("end_at must be after start_at".into()));
    }
    let w = rampart_db::maintenance::create(s.pool(), input).await?;
    Ok((StatusCode::CREATED, Json(w)))
}

async fn remove(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    rampart_db::maintenance::delete(s.pool(), parse_id(&id)?).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(serde::Deserialize)]
struct SetActiveBody {
    active: bool,
}

async fn set_active(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SetActiveBody>,
) -> Result<StatusCode, ApiError> {
    rampart_db::maintenance::set_active(s.pool(), parse_id(&id)?, body.active).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn attach(
    State(s): State<AppState>,
    Path((id, monitor_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    rampart_db::maintenance::attach(s.pool(), parse_id(&id)?, parse_monitor(&monitor_id)?).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn detach(
    State(s): State<AppState>,
    Path((id, monitor_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    rampart_db::maintenance::detach(s.pool(), parse_id(&id)?, parse_monitor(&monitor_id)?).await?;
    Ok(StatusCode::NO_CONTENT)
}
