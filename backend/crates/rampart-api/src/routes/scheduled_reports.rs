//! `/v1/scheduled-reports` routes.
//!
//! Admin CRUD for scheduled weekly uptime reports. A report is a named set
//! of email recipients that receives a per-monitor 7-day uptime digest on
//! a cadence (weekly today). The scheduler's slow-tick check renders +
//! sends due reports via the system SMTP sender; these routes only manage
//! the rows. Admin-gated via the `require_admin` layer in routes/mod.rs.

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use rampart_core::scheduled_report::ScheduledReport;
use rampart_core::ScheduledReportId;
use rampart_db::scheduled_reports::{NewScheduledReport, UpdateScheduledReport};
use std::str::FromStr;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", get(get_one).patch(update).delete(remove))
}

fn parse(id: &str) -> Result<ScheduledReportId, ApiError> {
    Uuid::from_str(id)
        .map(ScheduledReportId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid scheduled report id".into()))
}

async fn list(State(s): State<AppState>) -> Result<Json<Vec<ScheduledReport>>, ApiError> {
    Ok(Json(rampart_db::scheduled_reports::list(s.pool()).await?))
}

async fn get_one(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ScheduledReport>, ApiError> {
    Ok(Json(
        rampart_db::scheduled_reports::get(s.pool(), parse(&id)?).await?,
    ))
}

async fn create(
    State(s): State<AppState>,
    Json(input): Json<NewScheduledReport>,
) -> Result<(StatusCode, Json<ScheduledReport>), ApiError> {
    if input.name.trim().is_empty() {
        return Err(ApiError::BadRequest("name is required".into()));
    }
    let r = rampart_db::scheduled_reports::create(s.pool(), input).await?;
    Ok((StatusCode::CREATED, Json(r)))
}

async fn update(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<UpdateScheduledReport>,
) -> Result<Json<ScheduledReport>, ApiError> {
    Ok(Json(
        rampart_db::scheduled_reports::update(s.pool(), parse(&id)?, input).await?,
    ))
}

async fn remove(State(s): State<AppState>, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    rampart_db::scheduled_reports::delete(s.pool(), parse(&id)?).await?;
    Ok(StatusCode::NO_CONTENT)
}
