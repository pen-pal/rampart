//! Monitor groups + dependencies.
//!
//! Two router trees:
//!   * `router()` — `/v1/monitor-groups[/:id]` CRUD.
//!   * `dep_router()` — `/v1/monitors/:id/dependencies[/:parent_id]`
//!     attach / detach / list. Merged into the monitors router so the
//!     child-id is part of the URL path naturally.

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use rampart_core::ids::{MonitorGroupId, MonitorId};
use rampart_core::monitor_group::{MonitorGroup, NewMonitorGroup, UpdateMonitorGroup};
use serde::Serialize;
use std::str::FromStr;
use uuid::Uuid;
use validator::Validate;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/:id", axum::routing::patch(update).delete(delete_one))
}

pub fn dep_router() -> Router<AppState> {
    Router::new()
        .route("/:id/dependencies", get(list_deps))
        .route(
            "/:id/dependencies/:parent_id",
            post(attach_dep).delete(detach_dep),
        )
}

fn parse_group(s: &str) -> Result<MonitorGroupId, ApiError> {
    Uuid::from_str(s)
        .map(MonitorGroupId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid group id".into()))
}

fn parse_monitor(s: &str) -> Result<MonitorId, ApiError> {
    Uuid::from_str(s)
        .map(MonitorId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid monitor id".into()))
}

async fn list(State(s): State<AppState>) -> Result<Json<Vec<MonitorGroup>>, ApiError> {
    Ok(Json(rampart_db::monitor_groups::list(s.pool()).await?))
}

async fn create(
    State(s): State<AppState>,
    Json(input): Json<NewMonitorGroup>,
) -> Result<(StatusCode, Json<MonitorGroup>), ApiError> {
    input.validate()?;
    let g = rampart_db::monitor_groups::create(s.pool(), input).await?;
    Ok((StatusCode::CREATED, Json(g)))
}

async fn update(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<UpdateMonitorGroup>,
) -> Result<Json<MonitorGroup>, ApiError> {
    input.validate()?;
    Ok(Json(
        rampart_db::monitor_groups::update(s.pool(), parse_group(&id)?, input).await?,
    ))
}

async fn delete_one(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    rampart_db::monitor_groups::delete(s.pool(), parse_group(&id)?).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
struct DepList {
    parents: Vec<MonitorId>,
    children: Vec<MonitorId>,
}

async fn list_deps(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<DepList>, ApiError> {
    let mid = parse_monitor(&id)?;
    let parents = rampart_db::monitor_groups::parents_of(s.pool(), mid).await?;
    let children = rampart_db::monitor_groups::children_of(s.pool(), mid).await?;
    Ok(Json(DepList { parents, children }))
}

async fn attach_dep(
    State(s): State<AppState>,
    Path((id, parent_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let child = parse_monitor(&id)?;
    let parent = parse_monitor(&parent_id)?;
    rampart_db::monitor_groups::attach_dependency(s.pool(), child, parent).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn detach_dep(
    State(s): State<AppState>,
    Path((id, parent_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let child = parse_monitor(&id)?;
    let parent = parse_monitor(&parent_id)?;
    rampart_db::monitor_groups::detach_dependency(s.pool(), child, parent).await?;
    Ok(StatusCode::NO_CONTENT)
}
