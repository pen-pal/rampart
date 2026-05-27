//! `/v1/users` — admin-side user management + self-service password change.
//!
//! The admin half is gated by `require_admin` and exposes list / create /
//! set-admin / delete. The change-password route lives here too because
//! the routing already mounts under v1_protected; logically it's a user
//! resource even though only the caller can change their own password.

use crate::auth::{hash_password, verify_password};
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use rampart_core::ids::UserId;
use rampart_db::users::{NewUser, User};
use serde::Deserialize;
use std::str::FromStr;
use uuid::Uuid;

pub fn admin_router() -> Router<AppState> {
    // Mounted with .route_layer(require_admin) at the caller site.
    Router::new()
        .route("/", get(list).post(create))
        .route("/:id", axum::routing::delete(remove))
        .route("/:id/admin", post(set_admin))
}

pub fn self_router() -> Router<AppState> {
    // /v1/users/me/password — no admin gate; the caller is acting on
    // their own account.
    Router::new().route("/me/password", post(change_password))
}

fn parse(s: &str) -> Result<UserId, ApiError> {
    Uuid::from_str(s)
        .map(UserId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid user id".into()))
}

#[derive(Deserialize)]
struct CreateUserInput {
    email:    String,
    #[serde(default)]
    name:     Option<String>,
    password: String,
    #[serde(default)]
    is_admin: bool,
}

async fn list(State(s): State<AppState>) -> Result<Json<Vec<User>>, ApiError> {
    Ok(Json(rampart_db::users::list(s.pool()).await?))
}

async fn create(
    State(s): State<AppState>,
    Json(input): Json<CreateUserInput>,
) -> Result<(StatusCode, Json<User>), ApiError> {
    if input.password.len() < 10 {
        return Err(ApiError::BadRequest("password must be at least 10 characters".into()));
    }
    if !input.email.contains('@') {
        return Err(ApiError::BadRequest("email looks invalid".into()));
    }
    let hash = hash_password(&input.password)?;
    let u = rampart_db::users::create(
        s.pool(),
        NewUser {
            email: input.email,
            name: input.name,
            password_hash: hash,
            is_admin: input.is_admin,
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(u)))
}

#[derive(Deserialize)]
struct SetAdminInput { is_admin: bool }

async fn set_admin(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Extension(caller): Extension<User>,
    Json(body): Json<SetAdminInput>,
) -> Result<StatusCode, ApiError> {
    let target = parse(&id)?;
    if !body.is_admin && target == caller.id {
        // Refuse to demote yourself — would lock you out of admin entirely
        // if you're the only admin left. Force the second admin to do it.
        return Err(ApiError::BadRequest("you can't demote yourself".into()));
    }
    rampart_db::users::set_admin(s.pool(), target, body.is_admin).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Extension(caller): Extension<User>,
) -> Result<StatusCode, ApiError> {
    let target = parse(&id)?;
    if target == caller.id {
        return Err(ApiError::BadRequest("you can't delete yourself".into()));
    }
    rampart_db::users::delete(s.pool(), target).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct ChangePasswordInput {
    current_password: String,
    new_password:     String,
}

async fn change_password(
    State(s): State<AppState>,
    Extension(caller): Extension<User>,
    Json(input): Json<ChangePasswordInput>,
) -> Result<StatusCode, ApiError> {
    if input.new_password.len() < 10 {
        return Err(ApiError::BadRequest("new password must be at least 10 characters".into()));
    }
    let raw = rampart_db::users::get_by_email(s.pool(), &caller.email)
        .await
        .map_err(|_| ApiError::Unauthorized)?;
    if !verify_password(&input.current_password, &raw.password_hash) {
        return Err(ApiError::Unauthorized);
    }
    let hash = hash_password(&input.new_password)?;
    rampart_db::users::set_password(s.pool(), caller.id, &hash).await?;
    Ok(StatusCode::NO_CONTENT)
}
