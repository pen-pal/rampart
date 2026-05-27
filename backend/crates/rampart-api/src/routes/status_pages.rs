//! `/v1/status-pages` (admin CRUD) and `/v1/public/status-pages/:slug`
//! (unauthenticated public view).
//!
//! The admin half lives behind the session middleware just like every
//! other /v1 route. The public half intentionally does NOT — the page
//! is meant to be linked from external sites.

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use rampart_core::ids::StatusPageId;
use rampart_core::status_page::{
    NewStatusPage, PublicStatusPage, StatusPage, UpdateStatusPage,
};
use std::str::FromStr;
use uuid::Uuid;
use validator::Validate;

pub fn admin_router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/:id", get(get_one).patch(update).delete(remove))
}

pub fn public_router() -> Router<AppState> {
    Router::new().route("/:slug", get(public_view))
}

fn parse(id: &str) -> Result<StatusPageId, ApiError> {
    Uuid::from_str(id)
        .map(StatusPageId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid status page id".into()))
}

async fn list(State(s): State<AppState>) -> Result<Json<Vec<StatusPage>>, ApiError> {
    Ok(Json(rampart_db::status_pages::list(s.pool()).await?))
}

async fn get_one(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<StatusPage>, ApiError> {
    Ok(Json(
        rampart_db::status_pages::get(s.pool(), parse(&id)?).await?,
    ))
}

async fn create(
    State(s): State<AppState>,
    Json(input): Json<NewStatusPage>,
) -> Result<(StatusCode, Json<StatusPage>), ApiError> {
    input
        .validate()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let p = rampart_db::status_pages::create(s.pool(), input).await?;
    Ok((StatusCode::CREATED, Json(p)))
}

async fn update(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<UpdateStatusPage>,
) -> Result<Json<StatusPage>, ApiError> {
    Ok(Json(
        rampart_db::status_pages::update(s.pool(), parse(&id)?, input).await?,
    ))
}

async fn remove(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    rampart_db::status_pages::delete(s.pool(), parse(&id)?).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn public_view(
    State(s): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<PublicStatusPage>, ApiError> {
    Ok(Json(
        rampart_db::status_pages::public_view(s.pool(), &slug).await?,
    ))
}
