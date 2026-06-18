//! `/v1/deploy-markers` — point-in-time deploy/change annotations overlaid on
//! the metric + response-time charts. GET is readonly; POST/DELETE need editor
//! (enforced by the `require_write_or_readonly_get` layer on v1_protected).

use crate::auth::OrgContext;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use rampart_core::deploy_marker::{DeployMarker, NewDeployMarker};
use rampart_core::ids::DeployMarkerId;
use serde::Deserialize;
use std::str::FromStr;
use uuid::Uuid;
use validator::Validate;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", axum::routing::delete(remove))
}

#[derive(Deserialize)]
struct ListParams {
    #[serde(default = "default_hours")]
    hours: i32,
    #[serde(default)]
    service: Option<String>,
}
fn default_hours() -> i32 {
    24
}

async fn list(
    State(s): State<AppState>,
    Query(p): Query<ListParams>,
) -> Result<Json<Vec<DeployMarker>>, ApiError> {
    let service = p.service.as_deref().filter(|s| !s.is_empty());
    Ok(Json(
        rampart_db::deploy_markers::list_window(s.pool(), p.hours, service).await?,
    ))
}

async fn create(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Json(input): Json<NewDeployMarker>,
) -> Result<(StatusCode, Json<DeployMarker>), ApiError> {
    input
        .validate()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let marker = rampart_db::deploy_markers::create(s.pool(), input, org.org_id).await?;
    Ok((StatusCode::CREATED, Json(marker)))
}

async fn remove(State(s): State<AppState>, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    let id = Uuid::from_str(&id)
        .map(DeployMarkerId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid deploy-marker id".into()))?;
    rampart_db::deploy_markers::delete(s.pool(), id).await?;
    Ok(StatusCode::NO_CONTENT)
}
