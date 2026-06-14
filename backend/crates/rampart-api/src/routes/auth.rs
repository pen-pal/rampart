//! `/v1/auth` routes.
//!
//! POST   /register    First-run signup. Only succeeds when zero users
//!                     exist; subsequent requests get 409. Returns the
//!                     new user + sets a session cookie.
//! POST   /login       Verify email/password, set session cookie.
//! POST   /logout      Delete the session row + clear the cookie.
//! GET    /me          Return the currently logged-in user, or:
//!                       - { needs_setup: true } when the DB has no users
//!                         (so the SPA can route to the first-run screen)
//!                       - 401 when authenticated would be expected.

use crate::auth::{
    build_clear_cookie, build_session_cookie, hash_password, is_secure, verify_password, AuthUser,
    SESSION_COOKIE, SESSION_TTL_SECS,
};
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{AppendHeaders, IntoResponse};
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::cookie::CookieJar;
use rampart_db::users::{NewUser, User};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::str::FromStr;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/me", get(me))
}

#[derive(Debug, Deserialize)]
pub struct RegisterInput {
    pub email: String,
    #[serde(default)]
    pub name: Option<String>,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginInput {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
struct AuthResponse {
    user: User,
}

async fn register(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<RegisterInput>,
) -> Result<impl IntoResponse, ApiError> {
    if input.password.len() < 10 {
        return Err(ApiError::BadRequest(
            "password must be at least 10 characters".into(),
        ));
    }
    if !input.email.contains('@') {
        return Err(ApiError::BadRequest("email looks invalid".into()));
    }

    // First-run only: once any user exists, registration is locked. Adding
    // additional users will happen through an admin-only flow later.
    let existing = rampart_db::users::count(state.pool()).await?;
    if existing > 0 {
        return Err(ApiError::Conflict(
            "registration is closed — a user already exists".into(),
        ));
    }

    let hash = hash_password(&input.password)?;
    let user = rampart_db::users::create(
        state.pool(),
        NewUser {
            email: input.email,
            name: input.name,
            password_hash: hash,
            role: rampart_core::Role::Admin, // the first user becomes admin
        },
    )
    .await?;
    rampart_db::users::mark_login(state.pool(), user.id)
        .await
        .ok();

    let session = rampart_db::sessions::create(
        state.pool(),
        user.id,
        SESSION_TTL_SECS,
        None,
        headers
            .get("user-agent")
            .and_then(|v| v.to_str().ok())
            .map(String::from),
    )
    .await?;

    let cookie = build_session_cookie(session.id, is_secure(&headers));
    Ok((
        StatusCode::CREATED,
        AppendHeaders([(axum::http::header::SET_COOKIE, cookie.to_string())]),
        Json(AuthResponse { user }),
    ))
}

async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<LoginInput>,
) -> Result<axum::response::Response, ApiError> {
    // Always do the password verify even if the user isn't found, to keep
    // timing roughly constant against email-enumeration probes. We hash
    // against a placeholder if there's no row.
    let lookup = rampart_db::users::get_by_email(state.pool(), &input.email).await;
    let (user_id, totp_enabled, ok) = match lookup {
        Ok(u) => (
            Some(u.id),
            u.totp_enabled,
            verify_password(&input.password, &u.password_hash),
        ),
        Err(_) => {
            // Spend roughly the same CPU verifying against a known hash.
            let _ = verify_password(
                &input.password,
                "$argon2id$v=19$m=19456,t=2,p=1$Y2FrZWlzYWxpZQ$dummyhashtomimiccostforabsentuser",
            );
            (None, false, false)
        }
    };

    if !ok || user_id.is_none() {
        // Security event: failed password auth. No trusted identity, so
        // record anonymously — the source IP + attempted email are the
        // forensic signal for brute-force / credential-stuffing review.
        crate::audit::record_anon(
            state.pool(),
            &headers,
            "auth.login_failed",
            "session",
            Some(json!({ "email": input.email })),
        )
        .await;
        return Err(ApiError::Unauthorized);
    }
    let user_id = user_id.unwrap();

    // 2FA gate: defer the session until the user proves they hold the
    // shared secret (or burns a recovery code).
    if totp_enabled {
        let challenge = state.issue_totp_challenge(user_id).await;
        return Ok((
            StatusCode::ACCEPTED,
            Json(json!({
                "totp_required":   true,
                "challenge_token": challenge.to_string(),
            })),
        )
            .into_response());
    }

    rampart_db::users::mark_login(state.pool(), user_id)
        .await
        .ok();
    let user = rampart_db::users::get(state.pool(), user_id).await?;
    crate::audit::record(
        state.pool(),
        &user,
        &headers,
        "auth.login",
        "session",
        None,
        None,
    )
    .await;

    let session = rampart_db::sessions::create(
        state.pool(),
        user_id,
        SESSION_TTL_SECS,
        None,
        headers
            .get("user-agent")
            .and_then(|v| v.to_str().ok())
            .map(String::from),
    )
    .await?;

    let cookie = build_session_cookie(session.id, is_secure(&headers));
    Ok((
        AppendHeaders([(axum::http::header::SET_COOKIE, cookie.to_string())]),
        Json(AuthResponse { user }),
    )
        .into_response())
}

async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
) -> impl IntoResponse {
    if let Some(token) = jar
        .get(SESSION_COOKIE)
        .and_then(|c| Uuid::from_str(c.value()).ok())
    {
        let _ = rampart_db::sessions::delete(state.pool(), token).await;
    }
    let cookie = build_clear_cookie(is_secure(&headers));
    (
        StatusCode::NO_CONTENT,
        AppendHeaders([(axum::http::header::SET_COOKIE, cookie.to_string())]),
    )
}

async fn me(State(state): State<AppState>, jar: CookieJar) -> Result<impl IntoResponse, ApiError> {
    // Special case: no users yet. The frontend uses this to decide whether
    // to show the login screen or the first-run signup screen.
    let count = rampart_db::users::count(state.pool()).await?;
    if count == 0 {
        return Ok(Json(json!({ "needs_setup": true })));
    }

    // Past first-run: behave like a normal protected endpoint.
    let token = jar
        .get(SESSION_COOKIE)
        .and_then(|c| Uuid::from_str(c.value()).ok())
        .ok_or(ApiError::Unauthorized)?;
    let session = rampart_db::sessions::get(state.pool(), token)
        .await
        .map_err(|_| ApiError::Unauthorized)?;
    let user = rampart_db::users::get(state.pool(), session.user_id)
        .await
        .map_err(|_| ApiError::Unauthorized)?;

    Ok(Json(json!({ "user": user })))
}

// Silence the unused-import warning if we later refactor.
#[allow(dead_code)]
fn _drop(_a: AuthUser) {}
