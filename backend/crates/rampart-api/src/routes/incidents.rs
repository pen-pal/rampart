//! Incidents — status-page announcements.
//!
//! Two mounting points:
//!
//! - `/v1/status-pages/:page_id/incidents` (page-scoped)
//!   GET → list (all, including resolved)
//!   POST → create + return
//! - `/v1/incidents/:id` (top-level operations on a single incident)
//!   PATCH → update title / content / style / pinned
//!   DELETE → remove (and cascade its updates)
//!   POST /resolve → mark resolved
//!   GET  /updates → list running updates
//!   POST /updates → append running update

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use rampart_core::ids::{IncidentId, StatusPageId};
use rampart_core::{Incident, IncidentUpdate};
use rampart_db::incidents::{NewIncident, UpdateIncident};
use rampart_db::users::User;
use serde::Deserialize;
use std::str::FromStr;
use time::OffsetDateTime;
use uuid::Uuid;

pub fn page_router() -> Router<AppState> {
    Router::new()
        .route("/:page_id/incidents", get(list_for_page).post(create))
}

pub fn incident_router() -> Router<AppState> {
    Router::new()
        .route("/:id", axum::routing::patch(update).delete(delete_one))
        .route("/:id/resolve", post(resolve))
        .route("/:id/updates", get(list_updates).post(post_update))
}

fn parse_page(s: &str) -> Result<StatusPageId, ApiError> {
    Uuid::from_str(s)
        .map(StatusPageId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid status page id".into()))
}
fn parse_incident(s: &str) -> Result<IncidentId, ApiError> {
    Uuid::from_str(s)
        .map(IncidentId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid incident id".into()))
}

async fn list_for_page(
    State(s): State<AppState>,
    Path(page): Path<String>,
) -> Result<Json<Vec<Incident>>, ApiError> {
    Ok(Json(
        rampart_db::incidents::list_all(s.pool(), parse_page(&page)?).await?,
    ))
}

async fn create(
    State(s): State<AppState>,
    Path(page): Path<String>,
    Extension(user): Extension<User>,
    Json(input): Json<NewIncident>,
) -> Result<(StatusCode, Json<Incident>), ApiError> {
    if input.title.trim().is_empty() {
        return Err(ApiError::BadRequest("title is required".into()));
    }
    let i = rampart_db::incidents::create(s.pool(), parse_page(&page)?, Some(user.id), input).await?;
    Ok((StatusCode::CREATED, Json(i)))
}

async fn update(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(patch): Json<UpdateIncident>,
) -> Result<Json<Incident>, ApiError> {
    Ok(Json(
        rampart_db::incidents::update(s.pool(), parse_incident(&id)?, patch).await?,
    ))
}

async fn delete_one(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    rampart_db::incidents::delete(s.pool(), parse_incident(&id)?).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn resolve(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    rampart_db::incidents::resolve(
        s.pool(),
        parse_incident(&id)?,
        OffsetDateTime::now_utc(),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_updates(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<IncidentUpdate>>, ApiError> {
    Ok(Json(
        rampart_db::incidents::list_updates(s.pool(), parse_incident(&id)?).await?,
    ))
}

#[derive(Deserialize)]
struct UpdateBody { message: String }

async fn post_update(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Extension(user): Extension<User>,
    Json(body): Json<UpdateBody>,
) -> Result<StatusCode, ApiError> {
    if body.message.trim().is_empty() {
        return Err(ApiError::BadRequest("message is required".into()));
    }
    rampart_db::incidents::post_update(
        s.pool(),
        parse_incident(&id)?,
        Some(user.id),
        body.message,
    )
    .await?;
    Ok(StatusCode::CREATED)
}
