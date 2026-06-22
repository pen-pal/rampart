//! `/v1/monitor-templates` routes.
//!
//! A monitor template is a named, reusable WHOLE monitor spec — distinct
//! from a monitor preset (a small saved header/TLS config slice). The
//! operator saves the full monitor-creation body under a name, then
//! `POST /{id}/instantiate` mints a brand-new monitor from it (optionally
//! overriding the name), reusing the same monitor-create path the regular
//! create endpoint uses. CRUD without PATCH — like presets, a template is
//! a bag the operator deletes + recreates rather than edits in place.
//!
//! Same RBAC slice as monitors (`editor_or_read`): readonly users GET,
//! editors + admins mutate.

use crate::auth::OrgContext;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use rampart_core::monitor::NewMonitor;
use rampart_core::{Monitor, MonitorTemplateId};
use rampart_db::monitor_templates::{MonitorTemplate, NewMonitorTemplate};
use rampart_db::users::User;
use serde::Deserialize;
use std::str::FromStr;
use uuid::Uuid;
use validator::Validate;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", get(get_one).delete(delete_one))
        .route("/{id}/instantiate", post(instantiate))
}

fn parse_template_id(s: &str) -> Result<MonitorTemplateId, ApiError> {
    Uuid::from_str(s)
        .map(MonitorTemplateId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid template id".into()))
}

async fn list(
    State(state): State<AppState>,
    Extension(org): Extension<OrgContext>,
) -> Result<Json<Vec<MonitorTemplate>>, ApiError> {
    Ok(Json(
        state.store().list_monitor_templates(org.org_id).await?,
    ))
}

async fn get_one(
    State(state): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Path(id): Path<String>,
) -> Result<Json<MonitorTemplate>, ApiError> {
    Ok(Json(
        state
            .store()
            .get_monitor_template(parse_template_id(&id)?, org.org_id)
            .await?,
    ))
}

async fn create(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Extension(org): Extension<OrgContext>,
    headers: HeaderMap,
    Json(mut input): Json<NewMonitorTemplate>,
) -> Result<(StatusCode, Json<MonitorTemplate>), ApiError> {
    input.name = input.name.trim().to_string();
    if input.name.is_empty() {
        return Err(ApiError::BadRequest("name is required".into()));
    }
    if !input.spec.is_object() {
        return Err(ApiError::BadRequest("spec must be a JSON object".into()));
    }
    // Validate the spec is a usable monitor body up front so a template
    // can't store something that will fail every instantiate.
    let spec: NewMonitor = serde_json::from_value(input.spec.clone())
        .map_err(|e| ApiError::BadRequest(format!("invalid monitor spec: {e}")))?;
    spec.validate()?;

    let template = state
        .store()
        .create_monitor_template(input, org.org_id)
        .await?;
    crate::audit::record(
        state.store(),
        &user,
        &headers,
        "monitor_template.create",
        "monitor_template",
        Some(template.id.0),
        Some(serde_json::json!({ "name": template.name })),
    )
    .await;
    Ok((StatusCode::CREATED, Json(template)))
}

async fn delete_one(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Extension(org): Extension<OrgContext>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let template_id = parse_template_id(&id)?;
    state
        .store()
        .delete_monitor_template(template_id, org.org_id)
        .await?;
    crate::audit::record(
        state.store(),
        &user,
        &headers,
        "monitor_template.delete",
        "monitor_template",
        Some(template_id.0),
        None,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// Optional body for `POST /v1/monitor-templates/{id}/instantiate`.
/// `name` overrides the spec's stored name; the whole body may be omitted.
#[derive(Debug, Default, Deserialize)]
pub struct InstantiateRequest {
    name: Option<String>,
}

/// Create a brand-new monitor from a stored template's spec. The spec is
/// fed through the same `rampart_db::monitors::create` path as a regular
/// create, so the resulting monitor is indistinguishable from a hand-made
/// one. An optional `name` in the body overrides the spec's stored name.
async fn instantiate(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Extension(org): Extension<OrgContext>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Option<Json<InstantiateRequest>>,
) -> Result<(StatusCode, Json<Monitor>), ApiError> {
    let template_id = parse_template_id(&id)?;
    let template = state
        .store()
        .get_monitor_template(template_id, org.org_id)
        .await?;

    let mut new_monitor: NewMonitor = serde_json::from_value(template.spec)
        .map_err(|e| ApiError::BadRequest(format!("invalid monitor spec: {e}")))?;

    let req = body.map(|Json(b)| b).unwrap_or_default();
    if let Some(name) = req.name.map(|n| n.trim().to_string()) {
        if !name.is_empty() {
            new_monitor.name = name;
        }
    }

    new_monitor.validate()?;
    let monitor = rampart_db::monitors::create(state.pool(), new_monitor, org.org_id).await?;
    state.poke_scheduler();
    crate::audit::record(
        state.store(),
        &user,
        &headers,
        "monitor_template.instantiate",
        "monitor",
        Some(monitor.id.0),
        Some(serde_json::json!({
            "template": template_id.0,
            "name": monitor.name,
        })),
    )
    .await;
    Ok((StatusCode::CREATED, Json(monitor)))
}
