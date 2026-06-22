//! `/v1/users` — admin-side user management + self-service password change.
//!
//! The admin half is gated by `require_admin` and exposes list / create /
//! set-admin / delete. The change-password route lives here too because
//! the routing already mounts under v1_protected; logically it's a user
//! resource even though only the caller can change their own password.

use crate::auth::{
    build_session_cookie, hash_password, is_secure, verify_password, SESSION_TTL_SECS,
};
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{AppendHeaders, IntoResponse};
use axum::routing::{get, post};
use axum::{Json, Router};
use rampart_core::ids::UserId;
use rampart_core::Role;
use rampart_db::users::{NewUser, User};
use serde::Deserialize;
use std::str::FromStr;
use uuid::Uuid;

pub fn admin_router() -> Router<AppState> {
    // Mounted with .route_layer(require_admin) at the caller site.
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", axum::routing::delete(remove))
        .route("/{id}/admin", post(set_admin))
        .route("/{id}/role", axum::routing::patch(set_role))
        .route("/{id}/export", get(export_data))
        .route("/{id}/erase", post(erase))
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
    email: String,
    #[serde(default)]
    name: Option<String>,
    password: String,
    /// New canonical field. Defaults to `editor` if omitted.
    #[serde(default)]
    role: Option<Role>,
    /// Legacy field kept for older clients: `is_admin: true` maps to the
    /// admin role when `role` isn't supplied.
    #[serde(default)]
    is_admin: bool,
}

async fn list(State(s): State<AppState>) -> Result<Json<Vec<User>>, ApiError> {
    Ok(Json(s.store().list_users().await?))
}

async fn create(
    State(s): State<AppState>,
    Extension(caller): Extension<User>,
    headers: HeaderMap,
    Json(input): Json<CreateUserInput>,
) -> Result<(StatusCode, Json<User>), ApiError> {
    crate::auth::validate_password(&input.password, &input.email)?;
    if !input.email.contains('@') {
        return Err(ApiError::BadRequest("email looks invalid".into()));
    }
    // Resolve the role: explicit `role` wins; otherwise fall back to the
    // legacy `is_admin` boolean (admin if true, editor if false).
    let role = input.role.unwrap_or(if input.is_admin {
        Role::Admin
    } else {
        Role::Editor
    });
    let hash = hash_password(&input.password)?;
    let u = s
        .store()
        .create_user(NewUser {
            email: input.email.clone(),
            name: input.name,
            password_hash: hash,
            role,
        })
        .await?;
    crate::audit::record(
        s.store(),
        &caller,
        &headers,
        "user.create",
        "user",
        Some(u.id.0),
        Some(serde_json::json!({ "email": input.email, "role": role })),
    )
    .await;
    Ok((StatusCode::CREATED, Json(u)))
}

#[derive(Deserialize)]
struct SetAdminInput {
    is_admin: bool,
}

async fn set_admin(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Extension(caller): Extension<User>,
    headers: HeaderMap,
    Json(body): Json<SetAdminInput>,
) -> Result<StatusCode, ApiError> {
    let target = parse(&id)?;
    if !body.is_admin && target == caller.id {
        // Refuse to demote yourself — would lock you out of admin entirely
        // if you're the only admin left. Force the second admin to do it.
        return Err(ApiError::BadRequest("you can't demote yourself".into()));
    }
    s.store().set_user_admin(target, body.is_admin).await?;
    crate::audit::record(
        s.store(),
        &caller,
        &headers,
        if body.is_admin {
            "user.promote"
        } else {
            "user.demote"
        },
        "user",
        Some(target.0),
        None,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct SetRoleInput {
    role: Role,
}

async fn set_role(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Extension(caller): Extension<User>,
    headers: HeaderMap,
    Json(body): Json<SetRoleInput>,
) -> Result<StatusCode, ApiError> {
    let target = parse(&id)?;
    // Refuse to drop your own admin — prevents the last admin locking
    // themselves out. Same guard the boolean demote path uses.
    if !body.role.is_admin() && target == caller.id {
        return Err(ApiError::BadRequest(
            "you can't remove your own admin role".into(),
        ));
    }
    s.store().set_user_role(target, body.role).await?;
    crate::audit::record(
        s.store(),
        &caller,
        &headers,
        "user.set_role",
        "user",
        Some(target.0),
        Some(serde_json::json!({ "role": body.role })),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Extension(caller): Extension<User>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let target = parse(&id)?;
    if target == caller.id {
        return Err(ApiError::BadRequest("you can't delete yourself".into()));
    }
    s.store().delete_user(target).await?;
    crate::audit::record(
        s.store(),
        &caller,
        &headers,
        "user.delete",
        "user",
        Some(target.0),
        None,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// GDPR data-subject access request: aggregate a user's personal data across
/// tables into one JSON document (admin-gated; the export itself is audited).
/// Covers the account profile, UI preferences, active sessions (IP/UA/time),
/// and org memberships — the personal data Rampart holds about the account.
async fn export_data(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Extension(caller): Extension<User>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let target = parse(&id)?;
    let user = s.store().get_user(target).await?;
    let preferences = s
        .store()
        .get_user_prefs(target)
        .await
        .unwrap_or_else(|_| serde_json::json!({}));
    let sessions = rampart_db::sessions::list_for_user(s.pool(), target)
        .await
        .unwrap_or_default();
    let organizations = s.store().orgs_for_user(target).await.unwrap_or_default();
    crate::audit::record(
        s.store(),
        &caller,
        &headers,
        "user.gdpr_export",
        "user",
        Some(target.0),
        None,
    )
    .await;
    Ok(Json(serde_json::json!({
        "user": user,
        "preferences": preferences,
        "sessions": sessions,
        "organizations": organizations,
    })))
}

/// GDPR right-to-erasure: scrub the user's PII in place (anonymize — NOT a hard
/// delete, which would break the tamper-evident audit chain + FK refs) and
/// revoke all access (sessions + recovery codes). The erasure action is itself
/// audited. The user row survives as an anonymized tombstone preserving audit
/// integrity (security-log legal-retention exception).
async fn erase(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Extension(caller): Extension<User>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let target = parse(&id)?;
    if target == caller.id {
        return Err(ApiError::BadRequest(
            "you can't erase your own account".into(),
        ));
    }
    s.store().anonymize_user(target).await?;
    let _ = rampart_db::sessions::delete_for_user(s.pool(), target).await;
    let _ = rampart_db::recovery_codes::delete_for_user(s.pool(), target).await;
    crate::audit::record(
        s.store(),
        &caller,
        &headers,
        "user.gdpr_erase",
        "user",
        Some(target.0),
        None,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct ChangePasswordInput {
    current_password: String,
    new_password: String,
}

async fn change_password(
    State(s): State<AppState>,
    Extension(caller): Extension<User>,
    headers: HeaderMap,
    Json(input): Json<ChangePasswordInput>,
) -> Result<impl IntoResponse, ApiError> {
    crate::auth::validate_password(&input.new_password, &caller.email)?;
    let raw = s
        .store()
        .get_user_by_email(&caller.email)
        .await
        .map_err(|_| ApiError::Unauthorized)?;
    if !verify_password(&input.current_password, &raw.password_hash) {
        return Err(ApiError::Unauthorized);
    }
    let hash = hash_password(&input.new_password)?;
    // set_password revokes ALL of the user's sessions (including this one).
    s.store().set_user_password(caller.id, &hash).await?;
    // Re-issue a fresh session for the current device so the user isn't logged
    // out by their own password change — other devices stay revoked.
    let session = s
        .store()
        .create_session(
            caller.id,
            SESSION_TTL_SECS,
            crate::client_ip::from_headers(&headers),
            headers
                .get("user-agent")
                .and_then(|v| v.to_str().ok())
                .map(String::from),
        )
        .await?;
    let cookie = build_session_cookie(session.id, is_secure(&headers));
    Ok((
        StatusCode::NO_CONTENT,
        AppendHeaders([(axum::http::header::SET_COOKIE, cookie.to_string())]),
    ))
}
