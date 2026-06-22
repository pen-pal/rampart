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

/// Auth routes. `login` + `register` are the brute-forceable / write surface
/// and carry the per-IP rate limiter passed in. `me` + `logout` are cheap
/// session ops the SPA polls on every navigation (and the Security view fires
/// several per mount) — they are deliberately NOT rate limited, otherwise quick
/// navigation exhausts the 10-burst bucket and the resulting 429 bounces the
/// user back to `#/login` despite a valid session.
pub fn router(auth_rate_limiter: crate::rate_limit::IpRateLimiter) -> Router<AppState> {
    let limited = Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
        .route_layer(axum::middleware::from_fn_with_state(
            auth_rate_limiter,
            crate::rate_limit::enforce_ip_rate_limit,
        ));
    Router::new()
        .route("/me", get(me))
        .route("/logout", post(logout))
        .merge(limited)
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
    if !input.email.contains('@') {
        return Err(ApiError::BadRequest("email looks invalid".into()));
    }
    crate::auth::validate_password(&input.password, &input.email)?;

    // First-run only: once any user exists, registration is locked. Adding
    // additional users will happen through an admin-only flow later.
    let existing = state.store().count_users().await?;
    if existing > 0 {
        return Err(ApiError::Conflict(
            "registration is closed — a user already exists".into(),
        ));
    }

    let hash = hash_password(&input.password)?;
    let user = state
        .store()
        .create_user(NewUser {
            email: input.email,
            name: input.name,
            password_hash: hash,
            role: rampart_core::Role::Admin, // the first user becomes admin
        })
        .await?;
    state.store().mark_user_login(user.id).await.ok();

    let session = state
        .store()
        .create_session(
            user.id,
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
    let lookup = state.store().get_user_by_email(&input.email).await;
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

    state.store().mark_user_login(user_id).await.ok();
    let user = state.store().get_user(user_id).await?;
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

    let session = state
        .store()
        .create_session(
            user_id,
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
        let _ = state.store().delete_session(token).await;
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
    let count = state.store().count_users().await?;
    if count == 0 {
        return Ok(Json(json!({ "needs_setup": true })));
    }

    // Past first-run: behave like a normal protected endpoint.
    let token = jar
        .get(SESSION_COOKIE)
        .and_then(|c| Uuid::from_str(c.value()).ok())
        .ok_or(ApiError::Unauthorized)?;
    let session = state
        .store()
        .lookup_session(token)
        .await
        .map_err(|_| ApiError::Unauthorized)?;
    let mut user = state
        .store()
        .get_user(session.user_id)
        .await
        .map_err(|_| ApiError::Unauthorized)?;

    // 2FA-enforcement policy (settings.require_2fa: off | admins | all). When it
    // applies to this user and they haven't enrolled, the SPA forces enrollment.
    let policy = state
        .store()
        .get_setting("require_2fa")
        .await
        .ok()
        .flatten()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "off".to_string());
    let applies = match policy.as_str() {
        "all" => true,
        "admins" => user.is_admin,
        _ => false,
    };
    let must_setup_2fa = applies && !user.totp_enabled;

    // Multi-tenancy (Phase 4d): surface the caller's org list + active org so the
    // SPA can render the org switcher. me() runs outside `require_session`, so it
    // re-resolves the active org from the session itself (Default-org fallback
    // when unset). Best-effort — a list failure mustn't break /me.
    let orgs = state
        .store()
        .orgs_for_user(user.id)
        .await
        .unwrap_or_default();
    let active_org_id = session
        .active_org_id
        .unwrap_or(rampart_core::org::DEFAULT_ORG_ID);

    // Effective role IN THE ACTIVE ORG — mirror require_session's resolution so the
    // SPA (which gates UI on user.role) sees exactly the role Phase 4e enforces.
    // Fall back to the Default-org role, then the global user.role, when the caller
    // is not a member of the active org (revoked / stale). `user.is_admin` stays the
    // GLOBAL flag (the 2FA `must_setup_2fa` logic above already keyed off it).
    let default_org = rampart_core::ids::OrgId::from_uuid(rampart_core::org::DEFAULT_ORG_ID);
    let want = rampart_core::ids::OrgId::from_uuid(active_org_id);
    let effective_role = match state.store().org_member_role(want, user.id).await {
        Ok(Some(r)) => r,
        _ => state
            .store()
            .org_member_role(default_org, user.id)
            .await
            .ok()
            .flatten()
            .unwrap_or(user.role),
    };
    user.role = effective_role;

    Ok(Json(json!({
        "user": user,
        "must_setup_2fa": must_setup_2fa,
        "orgs": orgs,
        "active_org_id": active_org_id,
    })))
}

// Silence the unused-import warning if we later refactor.
#[allow(dead_code)]
fn _drop(_a: AuthUser) {}
