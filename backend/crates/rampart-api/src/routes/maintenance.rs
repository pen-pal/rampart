//! `/v1/maintenance-windows` routes.

use crate::auth::OrgContext;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use rampart_core::ids::{MaintenanceId, MonitorId};
use rampart_core::maintenance::{MaintenanceWindow, NewMaintenanceWindow, UpdateMaintenanceWindow};
use rampart_db::users::User;
use std::str::FromStr;
use uuid::Uuid;
use validator::Validate;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", get(get_one).patch(update).delete(remove))
        .route("/{id}/active", post(set_active))
        .route("/{id}/monitors/{monitor_id}", post(attach).delete(detach))
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

async fn list(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
) -> Result<Json<Vec<MaintenanceWindow>>, ApiError> {
    Ok(Json(
        s.store().list_maintenance_windows(org.org_id).await?,
    ))
}

async fn get_one(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Path(id): Path<String>,
) -> Result<Json<MaintenanceWindow>, ApiError> {
    Ok(Json(
        s.store().get_maintenance_window(parse_id(&id)?, org.org_id).await?,
    ))
}

async fn create(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    Extension(org): Extension<OrgContext>,
    headers: HeaderMap,
    Json(input): Json<NewMaintenanceWindow>,
) -> Result<(StatusCode, Json<MaintenanceWindow>), ApiError> {
    input
        .validate()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    if input.end_at <= input.start_at {
        return Err(ApiError::BadRequest("end_at must be after start_at".into()));
    }
    let w = s.store().create_maintenance_window(input, org.org_id).await?;
    let rfc3339 = time::format_description::well_known::Rfc3339;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "maintenance.create",
        "maintenance_window",
        Some(w.id.0),
        Some(serde_json::json!({
            "name": w.name,
            "start_at": w.start_at.format(&rfc3339).unwrap_or_default(),
            "end_at":   w.end_at.format(&rfc3339).unwrap_or_default(),
        })),
    )
    .await;
    Ok((StatusCode::CREATED, Json(w)))
}

async fn update(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    Extension(org): Extension<OrgContext>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<UpdateMaintenanceWindow>,
) -> Result<Json<MaintenanceWindow>, ApiError> {
    input
        .validate()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let wid = parse_id(&id)?;
    let w = s.store().update_maintenance_window(wid, input, org.org_id).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "maintenance.update",
        "maintenance_window",
        Some(wid.0),
        Some(serde_json::json!({ "name": w.name })),
    )
    .await;
    Ok(Json(w))
}

async fn remove(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    Extension(org): Extension<OrgContext>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let wid = parse_id(&id)?;
    s.store().delete_maintenance_window(wid, org.org_id).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "maintenance.delete",
        "maintenance_window",
        Some(wid.0),
        None,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(serde::Deserialize)]
struct SetActiveBody {
    active: bool,
}

async fn set_active(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    Extension(org): Extension<OrgContext>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<SetActiveBody>,
) -> Result<StatusCode, ApiError> {
    let wid = parse_id(&id)?;
    s.store().set_active_maintenance(wid, body.active, org.org_id).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "maintenance.set_active",
        "maintenance_window",
        Some(wid.0),
        Some(serde_json::json!({ "active": body.active })),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

async fn attach(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    Extension(org): Extension<OrgContext>,
    headers: HeaderMap,
    Path((id, monitor_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let wid = parse_id(&id)?;
    let mid = parse_monitor(&monitor_id)?;
    // Org-gate both ends before linking them: each `get` 404s cross-org,
    // so a caller can't attach another org's monitor or touch another
    // org's window.
    s.store().get_maintenance_window(wid, org.org_id).await?;
    rampart_db::monitors::get(s.pool(), mid, org.org_id).await?;
    s.store().attach_maintenance_monitor(wid, mid).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "maintenance.attach",
        "maintenance_window",
        Some(wid.0),
        Some(serde_json::json!({ "monitor_id": mid.0 })),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

async fn detach(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    Extension(org): Extension<OrgContext>,
    headers: HeaderMap,
    Path((id, monitor_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let wid = parse_id(&id)?;
    let mid = parse_monitor(&monitor_id)?;
    // Org-gate both ends before unlinking: each `get` 404s cross-org.
    s.store().get_maintenance_window(wid, org.org_id).await?;
    rampart_db::monitors::get(s.pool(), mid, org.org_id).await?;
    s.store().detach_maintenance_monitor(wid, mid).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "maintenance.detach",
        "maintenance_window",
        Some(wid.0),
        Some(serde_json::json!({ "monitor_id": mid.0 })),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}
