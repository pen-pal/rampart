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
        .route(
            "/{id}/sourcemaps",
            get(list_sourcemaps).post(upload_sourcemap),
        )
        .route(
            "/{id}/sourcemaps/{map_id}",
            axum::routing::delete(delete_sourcemap),
        )
}

pub fn issue_router() -> Router<AppState> {
    Router::new()
        .route("/{id}", get(get_issue))
        .route("/{id}/events", get(list_events))
        .route("/{id}/stats", get(issue_stats))
        .route("/{id}/resolve", post(resolve))
        .route("/{id}/ignore", post(ignore))
        .route("/{id}/unresolve", post(unresolve))
        .route("/{id}/assign", post(assign))
        .route("/assignable-users", get(assignable_users))
        .route("/recent", get(recent_issues))
        .route("/by-trace/{trace_id}", get(issues_by_trace))
}

async fn recent_issues(State(s): State<AppState>) -> Result<Json<Vec<ErrorIssue>>, ApiError> {
    Ok(Json(
        rampart_db::error_tracking::recent_open_issues(s.pool(), 8).await?,
    ))
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
    Ok(Json(
        rampart_db::error_tracking::get_issue(s.pool(), iid).await?,
    ))
}

async fn issue_stats(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<rampart_db::error_tracking::IssueStats>, ApiError> {
    let iid = issue_id(&id)?;
    Ok(Json(
        rampart_db::error_tracking::issue_stats(s.pool(), iid).await?,
    ))
}

async fn list_events(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<ErrorEvent>>, ApiError> {
    let iid = issue_id(&id)?;
    let mut events = rampart_db::error_tracking::list_events(s.pool(), iid, 50).await?;
    symbolicate_events(s.pool(), &mut events).await;
    Ok(Json(events))
}

/// Attach a `resolved` block to each minified frame that has an uploaded source
/// map for its event's release + file basename. Best-effort: a missing/garbage
/// map just leaves the frame as-is. Maps are cached per (release, file) for the
/// batch so a 50-event page does one lookup per unique file.
async fn symbolicate_events(pool: &rampart_db::DbPool, events: &mut [ErrorEvent]) {
    use std::collections::HashMap;
    let mut cache: HashMap<(String, String), Option<serde_json::Value>> = HashMap::new();
    for ev in events.iter_mut() {
        let Some(release) = ev.release.clone() else {
            continue;
        };
        let pid = ev.project_id.0;
        let Some(frames) = ev.stacktrace.as_mut().and_then(|v| v.as_array_mut()) else {
            continue;
        };
        for frame in frames {
            let base = {
                let Some(file) = frame.get("filename").and_then(|v| v.as_str()) else {
                    continue;
                };
                crate::symbolicate::basename(file).to_string()
            };
            let Some(lineno) = frame.get("lineno").and_then(|v| v.as_i64()) else {
                continue;
            };
            let colno = frame.get("colno").and_then(|v| v.as_i64());
            let key = (release.clone(), base);
            if !cache.contains_key(&key) {
                let m = rampart_db::source_maps::get(pool, pid, &key.0, &key.1)
                    .await
                    .ok()
                    .flatten();
                cache.insert(key.clone(), m);
            }
            let Some(Some(map)) = cache.get(&key) else {
                continue;
            };
            if let Some(r) = crate::symbolicate::resolve(map, lineno, colno) {
                if let Some(obj) = frame.as_object_mut() {
                    if let Ok(v) = serde_json::to_value(r) {
                        obj.insert("resolved".into(), v);
                    }
                }
            }
        }
    }
}

#[derive(Deserialize)]
struct UploadSourceMap {
    release: String,
    filename: String,
    map: serde_json::Value,
}

async fn upload_sourcemap(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<UploadSourceMap>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let pid = project_id(&id)?;
    if input.release.trim().is_empty() || input.filename.trim().is_empty() {
        return Err(ApiError::BadRequest(
            "release and filename are required".into(),
        ));
    }
    let base = crate::symbolicate::basename(&input.filename).to_string();
    let map_id = rampart_db::source_maps::upsert(
        s.pool(),
        rampart_db::source_maps::NewSourceMap {
            project_id: pid.0,
            release: input.release.trim(),
            filename: &base,
            map: input.map,
        },
    )
    .await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "error_sourcemap.upload",
        "error_project",
        Some(pid.0),
        Some(serde_json::json!({ "release": input.release, "filename": base })),
    )
    .await;
    Ok(Json(serde_json::json!({ "id": map_id })))
}

async fn list_sourcemaps(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<rampart_db::source_maps::SourceMapMeta>>, ApiError> {
    let pid = project_id(&id)?;
    Ok(Json(rampart_db::source_maps::list(s.pool(), pid.0).await?))
}

async fn delete_sourcemap(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path((id, map_id)): Path<(String, i64)>,
) -> Result<StatusCode, ApiError> {
    let pid = project_id(&id)?;
    if !rampart_db::source_maps::delete(s.pool(), pid.0, map_id).await? {
        return Err(ApiError::NotFound);
    }
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "error_sourcemap.delete",
        "error_project",
        Some(pid.0),
        Some(serde_json::json!({ "map_id": map_id })),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
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

#[derive(Deserialize)]
struct AssignBody {
    /// User UUID to assign, or null/empty to unassign.
    #[serde(default)]
    assignee: Option<String>,
}

async fn assign(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<AssignBody>,
) -> Result<Json<ErrorIssue>, ApiError> {
    let iid = issue_id(&id)?;
    let assignee = match body.assignee.as_deref().filter(|v| !v.is_empty()) {
        Some(u) => Some(
            Uuid::from_str(u)
                .map(rampart_core::ids::UserId::from_uuid)
                .map_err(|_| ApiError::BadRequest("invalid user id".into()))?,
        ),
        None => None,
    };
    let issue = rampart_db::error_tracking::assign_issue(s.pool(), iid, assignee).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "error_issue.assign",
        "error_issue",
        Some(iid.0),
        Some(serde_json::json!({ "assignee": assignee.map(|u| u.0) })),
    )
    .await;
    Ok(Json(issue))
}

async fn assignable_users(
    State(s): State<AppState>,
) -> Result<Json<Vec<rampart_db::error_tracking::AssignableUser>>, ApiError> {
    Ok(Json(
        rampart_db::error_tracking::assignable_users(s.pool()).await?,
    ))
}

/// Error issues touched by a given trace — the trace → errors correlation link.
async fn issues_by_trace(
    State(s): State<AppState>,
    Path(trace_id): Path<String>,
) -> Result<Json<Vec<rampart_db::error_tracking::TraceErrorRef>>, ApiError> {
    Ok(Json(
        rampart_db::error_tracking::issues_for_trace(s.pool(), &trace_id).await?,
    ))
}
