//! `/v1/incident-templates` routes.
//!
//! CRUD for the global incident-update template library. Editors + admins
//! manage these (readonly users GET fine, blocked on mutation by the
//! `require_write_or_readonly_get` layer in routes/mod.rs). Templates are
//! global, not page-scoped, so there is no status_page_id in the path.

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use rampart_core::{IncidentTemplate, IncidentTemplateId};
use rampart_db::incident_templates::{NewIncidentTemplate, UpdateIncidentTemplate};
use std::str::FromStr;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", get(get_one).patch(update).delete(remove))
}

fn parse(id: &str) -> Result<IncidentTemplateId, ApiError> {
    Uuid::from_str(id)
        .map(IncidentTemplateId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid incident template id".into()))
}

async fn list(State(s): State<AppState>) -> Result<Json<Vec<IncidentTemplate>>, ApiError> {
    Ok(Json(rampart_db::incident_templates::list(s.pool()).await?))
}

async fn get_one(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<IncidentTemplate>, ApiError> {
    Ok(Json(
        rampart_db::incident_templates::get(s.pool(), parse(&id)?).await?,
    ))
}

async fn create(
    State(s): State<AppState>,
    Json(input): Json<NewIncidentTemplate>,
) -> Result<(StatusCode, Json<IncidentTemplate>), ApiError> {
    if input.name.trim().is_empty() {
        return Err(ApiError::BadRequest("name is required".into()));
    }
    if input.body.trim().is_empty() {
        return Err(ApiError::BadRequest("body is required".into()));
    }
    let t = rampart_db::incident_templates::create(s.pool(), input).await?;
    Ok((StatusCode::CREATED, Json(t)))
}

async fn update(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<UpdateIncidentTemplate>,
) -> Result<Json<IncidentTemplate>, ApiError> {
    Ok(Json(
        rampart_db::incident_templates::update(s.pool(), parse(&id)?, input).await?,
    ))
}

async fn remove(State(s): State<AppState>, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    rampart_db::incident_templates::delete(s.pool(), parse(&id)?).await?;
    Ok(StatusCode::NO_CONTENT)
}
