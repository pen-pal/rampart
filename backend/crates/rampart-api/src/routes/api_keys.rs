//! `/v1/api-keys` — list / create / revoke.
//!
//! Create returns the raw token exactly once. The list endpoint only
//! exposes the prefix; the secret half never leaves the server again.

use crate::auth::OrgContext;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use axum::{Json, Router};
use rampart_core::api_key::{ApiKey, IssuedApiKey, NewApiKey};
use rampart_core::ids::ApiKeyId;
use rampart_db::users::User;
use std::str::FromStr;
use time::OffsetDateTime;
use uuid::Uuid;
use validator::Validate;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", axum::routing::delete(revoke))
}

fn parse(s: &str) -> Result<ApiKeyId, ApiError> {
    Uuid::from_str(s)
        .map(ApiKeyId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid api key id".into()))
}

async fn list(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
) -> Result<Json<Vec<ApiKey>>, ApiError> {
    Ok(Json(s.store().list_api_keys(org.org_id).await?))
}

async fn create(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    Extension(org): Extension<OrgContext>,
    headers: HeaderMap,
    Json(input): Json<NewApiKey>,
) -> Result<(StatusCode, Json<IssuedApiKey>), ApiError> {
    input
        .validate()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    if let Some(exp) = input.expires_at {
        if exp <= OffsetDateTime::now_utc() {
            return Err(ApiError::BadRequest(
                "expires_at must be in the future".into(),
            ));
        }
    }
    let name = input.name.clone();
    let issued = s.store().create_api_key(input, user.id, org.org_id).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "api_key.create",
        "api_key",
        Some(issued.key.id.0),
        Some(serde_json::json!({ "name": name })),
    )
    .await;
    Ok((StatusCode::CREATED, Json(issued)))
}

async fn revoke(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    Extension(org): Extension<OrgContext>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let key_id = parse(&id)?;
    s.store().delete_api_key(key_id, org.org_id).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "api_key.revoke",
        "api_key",
        Some(key_id.0),
        None,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}
