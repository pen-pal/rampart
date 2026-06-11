//! Error-tracking admin API (editor-gated).
//!
//! /v1/error-projects              — project CRUD (create mints the DSN key)
//! /v1/error-projects/{id}/issues  — issue list (optional ?status=)
//! /v1/error-issues/{id}           — issue detail
//! /v1/error-issues/{id}/events    — recent events for an issue
//! /v1/error-issues/{id}/{resolve|ignore|unresolve} — status changes
//!
//! Ingest (Sentry-compatible, DSN-keyed) lives in `error_ingest`, mounted at
//! the root `/api` surface outside the session layer.

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use rampart_core::error_tracking::{
    ErrorEvent, ErrorIssue, ErrorProject, NewErrorProject, UpdateErrorProject,
};
use rampart_core::ids::{ErrorIssueId, ErrorProjectId};
use rampart_db::users::User;
use serde::Deserialize;
use std::str::FromStr;
use uuid::Uuid;
use validator::Validate;

pub fn project_router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_projects).post(create_project))
        .route(
            "/{id}",
            axum::routing::patch(update_project).delete(delete_project),
        )
        .route("/{id}/issues", get(list_issues))
}

pub fn issue_router() -> Router<AppState> {
    Router::new()
        .route("/{id}", get(get_issue))
        .route("/{id}/events", get(list_events))
        .route("/{id}/resolve", post(resolve))
        .route("/{id}/ignore", post(ignore))
        .route("/{id}/unresolve", post(unresolve))
}

fn project_id(s: &str) -> Result<ErrorProjectId, ApiError> {
    Uuid::from_str(s)
        .map(ErrorProjectId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid project id".into()))
}

fn issue_id(s: &str) -> Result<ErrorIssueId, ApiError> {
    Uuid::from_str(s)
        .map(ErrorIssueId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid issue id".into()))
}

// ─────────────────────────── projects ───────────────────────────

async fn list_projects(State(s): State<AppState>) -> Result<Json<Vec<ErrorProject>>, ApiError> {
    Ok(Json(rampart_db::error_tracking::list(s.pool()).await?))
}

async fn create_project(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Json(input): Json<NewErrorProject>,
) -> Result<(StatusCode, Json<ErrorProject>), ApiError> {
    input
        .validate()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let name = input.name.clone();
    let project = rampart_db::error_tracking::create(s.pool(), input).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "error_project.create",
        "error_project",
        Some(project.id.0),
        Some(serde_json::json!({ "name": name })),
    )
    .await;
    Ok((StatusCode::CREATED, Json(project)))
}

async fn update_project(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<UpdateErrorProject>,
) -> Result<Json<ErrorProject>, ApiError> {
    let pid = project_id(&id)?;
    input
        .validate()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let project = rampart_db::error_tracking::update(s.pool(), pid, input).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "error_project.update",
        "error_project",
        Some(pid.0),
        None,
    )
    .await;
    Ok(Json(project))
}

async fn delete_project(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let pid = project_id(&id)?;
    rampart_db::error_tracking::delete(s.pool(), pid).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "error_project.delete",
        "error_project",
        Some(pid.0),
        None,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

// ─────────────────────────── issues ───────────────────────────

#[derive(Deserialize)]
struct IssueQuery {
    status: Option<String>,
}

async fn list_issues(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<IssueQuery>,
) -> Result<Json<Vec<ErrorIssue>>, ApiError> {
    let pid = project_id(&id)?;
    Ok(Json(
        rampart_db::error_tracking::list_issues(s.pool(), pid, q.status.as_deref()).await?,
    ))
}

async fn get_issue(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ErrorIssue>, ApiError> {
    let iid = issue_id(&id)?;
    Ok(Json(rampart_db::error_tracking::get_issue(s.pool(), iid).await?))
}

async fn list_events(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<ErrorEvent>>, ApiError> {
    let iid = issue_id(&id)?;
    Ok(Json(
        rampart_db::error_tracking::list_events(s.pool(), iid, 50).await?,
    ))
}

async fn set_status(
    s: &AppState,
    user: &User,
    headers: &HeaderMap,
    id: &str,
    status: &str,
) -> Result<Json<ErrorIssue>, ApiError> {
    let iid = issue_id(id)?;
    let issue = rampart_db::error_tracking::set_issue_status(s.pool(), iid, status).await?;
    crate::audit::record(
        s.pool(),
        user,
        headers,
        "error_issue.status",
        "error_issue",
        Some(iid.0),
        Some(serde_json::json!({ "status": status })),
    )
    .await;
    Ok(Json(issue))
}

async fn resolve(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<ErrorIssue>, ApiError> {
    set_status(&s, &user, &headers, &id, "resolved").await
}

async fn ignore(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<ErrorIssue>, ApiError> {
    set_status(&s, &user, &headers, &id, "ignored").await
}

async fn unresolve(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<ErrorIssue>, ApiError> {
    set_status(&s, &user, &headers, &id, "unresolved").await
}
