//! Monitor groups + dependencies.
//!
//! Two router trees:
//!   * `router()` — `/v1/monitor-groups[/:id]` CRUD.
//!   * `dep_router()` — `/v1/monitors/:id/dependencies[/:parent_id]`
//!     attach / detach / list. Merged into the monitors router so the
//!     child-id is part of the URL path naturally.

use crate::auth::OrgContext;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use rampart_core::ids::{MonitorGroupId, MonitorId};
use rampart_core::monitor_group::{MonitorGroup, NewMonitorGroup, UpdateMonitorGroup};
use rampart_db::users::User;
use serde::Serialize;
use std::str::FromStr;
use uuid::Uuid;
use validator::Validate;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", axum::routing::patch(update).delete(delete_one))
}

pub fn dep_router() -> Router<AppState> {
    Router::new()
        .route("/{id}/dependencies", get(list_deps))
        .route(
            "/{id}/dependencies/{parent_id}",
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

async fn list(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
) -> Result<Json<Vec<MonitorGroup>>, ApiError> {
    Ok(Json(
        rampart_db::monitor_groups::list(s.pool(), org.org_id).await?,
    ))
}

async fn create(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Json(input): Json<NewMonitorGroup>,
) -> Result<(StatusCode, Json<MonitorGroup>), ApiError> {
    input.validate()?;
    let g = rampart_db::monitor_groups::create(s.pool(), input).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "monitor_group.create",
        "monitor_group",
        Some(g.id.0),
        Some(serde_json::json!({ "name": g.name, "parent_id": g.parent_id })),
    )
    .await;
    Ok((StatusCode::CREATED, Json(g)))
}

async fn update(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    Extension(org): Extension<OrgContext>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<UpdateMonitorGroup>,
) -> Result<Json<MonitorGroup>, ApiError> {
    input.validate()?;
    let gid = parse_group(&id)?;
    let g = rampart_db::monitor_groups::update(s.pool(), gid, input, org.org_id).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "monitor_group.update",
        "monitor_group",
        Some(gid.0),
        Some(serde_json::json!({ "name": g.name, "parent_id": g.parent_id })),
    )
    .await;
    Ok(Json(g))
}

async fn delete_one(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    Extension(org): Extension<OrgContext>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let gid = parse_group(&id)?;
    rampart_db::monitor_groups::delete(s.pool(), gid, org.org_id).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "monitor_group.delete",
        "monitor_group",
        Some(gid.0),
        None,
    )
    .await;
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
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path((id, parent_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let child = parse_monitor(&id)?;
    let parent = parse_monitor(&parent_id)?;
    rampart_db::monitor_groups::attach_dependency(s.pool(), child, parent).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "monitor.attach_dependency",
        "monitor",
        Some(child.0),
        Some(serde_json::json!({ "depends_on": parent.0 })),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

async fn detach_dep(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path((id, parent_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let child = parse_monitor(&id)?;
    let parent = parse_monitor(&parent_id)?;
    rampart_db::monitor_groups::detach_dependency(s.pool(), child, parent).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "monitor.detach_dependency",
        "monitor",
        Some(child.0),
        Some(serde_json::json!({ "depends_on": parent.0 })),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}
