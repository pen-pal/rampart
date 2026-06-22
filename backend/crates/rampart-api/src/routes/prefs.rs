//! `/v1/me/prefs` — self-service per-user UI preferences.
//!
//! GET  /v1/me/prefs        → the caller's preferences blob (`{}` when unset)
//! PUT  /v1/me/prefs { prefs } → replace the caller's preferences blob
//!
//! These live in the self-service router group (no admin / no write gate): a
//! readonly user is still allowed to manage their OWN dashboard views and
//! default folder. The backend treats the blob as opaque — it stores saved
//! filter views + the default folder id but never interprets the shape.

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Extension, State};
use axum::routing::get;
use axum::{Json, Router};
use rampart_db::users::User;
use serde::Deserialize;

pub fn router() -> Router<AppState> {
    Router::new().route("/prefs", get(get_prefs).put(set_prefs))
}

async fn get_prefs(
    State(s): State<AppState>,
    Extension(caller): Extension<User>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let prefs = s.store().get_user_prefs(caller.id).await?;
    Ok(Json(prefs))
}

#[derive(Deserialize)]
struct SetPrefsInput {
    prefs: serde_json::Value,
}

async fn set_prefs(
    State(s): State<AppState>,
    Extension(caller): Extension<User>,
    Json(input): Json<SetPrefsInput>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Guard against unbounded blobs — saved views are small, so a few KB is a
    // generous ceiling that still stops a client from stuffing megabytes into
    // a row that's read on every dashboard load.
    if input.prefs.to_string().len() > 64 * 1024 {
        return Err(ApiError::BadRequest("preferences blob too large".into()));
    }
    s.store().set_user_prefs(caller.id, &input.prefs).await?;
    Ok(Json(input.prefs))
}
