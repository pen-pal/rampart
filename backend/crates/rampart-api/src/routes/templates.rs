//! `/v1/notification-templates` routes.

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use rampart_core::ids::NotificationTemplateId;
use rampart_db::templates::{NewTemplate, Template, UpdateTemplate};
use std::str::FromStr;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/:id", get(get_one).patch(update).delete(remove))
}

fn parse(id: &str) -> Result<NotificationTemplateId, ApiError> {
    Uuid::from_str(id)
        .map(NotificationTemplateId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid template id".into()))
}

async fn list(State(s): State<AppState>) -> Result<Json<Vec<Template>>, ApiError> {
    Ok(Json(rampart_db::templates::list(s.pool()).await?))
}

async fn get_one(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Template>, ApiError> {
    Ok(Json(
        rampart_db::templates::get(s.pool(), parse(&id)?).await?,
    ))
}

async fn create(
    State(s): State<AppState>,
    Json(input): Json<NewTemplate>,
) -> Result<(StatusCode, Json<Template>), ApiError> {
    if input.name.trim().is_empty() {
        return Err(ApiError::BadRequest("name is required".into()));
    }
    if input.body_template.trim().is_empty() {
        return Err(ApiError::BadRequest("body_template is required".into()));
    }
    let t = rampart_db::templates::create(s.pool(), input).await?;
    Ok((StatusCode::CREATED, Json(t)))
}

async fn update(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<UpdateTemplate>,
) -> Result<Json<Template>, ApiError> {
    Ok(Json(
        rampart_db::templates::update(s.pool(), parse(&id)?, input).await?,
    ))
}

async fn remove(State(s): State<AppState>, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    rampart_db::templates::delete(s.pool(), parse(&id)?).await?;
    Ok(StatusCode::NO_CONTENT)
}
