//! Account sessions — list "where am I logged in" and revoke. Self-service:
//! every handler is scoped to the calling user, so one user can't see or kill
//! another's sessions. The current session is flagged so the UI can label it
//! and "revoke others" keeps it alive.

use crate::auth::SESSION_COOKIE;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use axum_extra::extract::cookie::CookieJar;
use rampart_db::users::User;
use serde::Serialize;
use std::str::FromStr;
use time::OffsetDateTime;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list))
        .route("/revoke-others", post(revoke_others))
        .route("/{id}", delete(revoke_one))
}

fn current_session(jar: &CookieJar) -> Option<Uuid> {
    jar.get(SESSION_COOKIE)
        .and_then(|c| Uuid::from_str(c.value()).ok())
}

#[derive(Serialize)]
struct SessionRow {
    id: Uuid,
    ip_addr: Option<String>,
    user_agent: Option<String>,
    created_at: OffsetDateTime,
    expires_at: OffsetDateTime,
    /// True for the session making this request.
    current: bool,
}

async fn list(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    jar: CookieJar,
) -> Result<Json<Vec<SessionRow>>, ApiError> {
    let cur = current_session(&jar);
    let rows = s.store().list_sessions_for_user(user.id).await?;
    Ok(Json(
        rows.into_iter()
            .map(|si| SessionRow {
                current: Some(si.id) == cur,
                id: si.id,
                ip_addr: si.ip_addr,
                user_agent: si.user_agent,
                created_at: si.created_at,
                expires_at: si.expires_at,
            })
            .collect(),
    ))
}

async fn revoke_one(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    if !s.store().delete_one_session_for_user(user.id, id).await? {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn revoke_others(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    jar: CookieJar,
) -> Result<Json<serde_json::Value>, ApiError> {
    let cur = current_session(&jar).ok_or(ApiError::Unauthorized)?;
    let revoked = s.store().delete_other_sessions(user.id, cur).await?;
    Ok(Json(serde_json::json!({ "revoked": revoked })))
}
