//! `/v1/tags` CRUD + attach/detach against monitors.

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use rampart_core::ids::{MonitorId, TagId};
use rampart_core::tag::{NewTag, Tag, TagBrief};
use std::str::FromStr;
use uuid::Uuid;
use validator::Validate;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/:id", axum::routing::delete(remove))
}

/// Sub-router mounted under /v1/monitors so the path is
/// /v1/monitors/:id/tags + /v1/monitors/:id/tags/:tag_id.
pub fn monitor_tag_router() -> Router<AppState> {
    Router::new()
        .route("/:id/tags", get(list_for_monitor))
        .route("/:id/tags/:tag_id", post(attach).delete(detach))
}

fn parse_tag(s: &str) -> Result<TagId, ApiError> {
    Uuid::from_str(s)
        .map(TagId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid tag id".into()))
}
fn parse_monitor(s: &str) -> Result<MonitorId, ApiError> {
    Uuid::from_str(s)
        .map(MonitorId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid monitor id".into()))
}

async fn list(State(s): State<AppState>) -> Result<Json<Vec<Tag>>, ApiError> {
    Ok(Json(rampart_db::tags::list(s.pool()).await?))
}

async fn create(
    State(s): State<AppState>,
    Json(input): Json<NewTag>,
) -> Result<(StatusCode, Json<Tag>), ApiError> {
    input
        .validate()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let t = rampart_db::tags::create(s.pool(), input).await?;
    Ok((StatusCode::CREATED, Json(t)))
}

async fn remove(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    rampart_db::tags::delete(s.pool(), parse_tag(&id)?).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_for_monitor(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<TagBrief>>, ApiError> {
    Ok(Json(
        rampart_db::tags::list_for_monitor(s.pool(), parse_monitor(&id)?).await?,
    ))
}

async fn attach(
    State(s): State<AppState>,
    Path((id, tag_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    rampart_db::tags::attach(s.pool(), parse_monitor(&id)?, parse_tag(&tag_id)?).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn detach(
    State(s): State<AppState>,
    Path((id, tag_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    rampart_db::tags::detach(s.pool(), parse_monitor(&id)?, parse_tag(&tag_id)?).await?;
    Ok(StatusCode::NO_CONTENT)
}
