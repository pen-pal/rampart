//! `/v1/auth/2fa/*` — TOTP enrolment, activation, deactivation, and
//! the second-step verifier used after a password challenge.
//!
//! Endpoints:
//!
//! - POST /v1/auth/2fa/setup — generate a fresh secret, return otpauth
//!   URI for QR rendering.
//! - POST /v1/auth/2fa/enable — confirm enrolment with a valid code,
//!   flip totp_enabled, issue + return one batch of recovery codes.
//! - POST /v1/auth/2fa/disable — verify current password + a current
//!   code, then wipe the secret and codes.
//! - POST /v1/auth/2fa/verify — used during login: complete the
//!   challenge with a TOTP or recovery code.

use crate::auth::{build_session_cookie, is_secure, verify_password, AuthUser};
use crate::error::ApiError;
use crate::state::AppState;
use crate::totp;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{AppendHeaders, IntoResponse};
use axum::routing::post;
use axum::{Json, Router};
use rampart_db::users::User;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

const SESSION_TTL_SECS: i64 = 60 * 60 * 24 * 14;

/// Consecutive failed 2FA codes before the account is locked out of the verify
/// step. 5 failures per 15-minute window puts a 6-digit-TOTP brute-force at
/// ~10^6 / (5*96/day) ≈ years — while leaving headroom for fat-fingers.
const TOTP_MAX_ATTEMPTS: i32 = 5;
/// Lockout duration once the cap is hit, in minutes.
const TOTP_LOCKOUT_MINS: i32 = 15;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/setup", post(setup))
        .route("/enable", post(enable))
        .route("/disable", post(disable))
}

/// Public router — verify sits outside the protected tree because the
/// caller doesn't have a session yet.
pub fn public_router() -> Router<AppState> {
    Router::new().route("/verify", post(verify))
}

#[derive(Serialize)]
struct SetupResp {
    secret: String,
    otpauth_uri: String,
}

async fn setup(State(state): State<AppState>, user: AuthUser) -> Result<Json<SetupResp>, ApiError> {
    let secret = totp::generate_secret();
    let uri = totp::provisioning_uri(&secret, &user.0.email)
        .map_err(|e| ApiError::BadRequest(format!("totp setup: {e}")))?;
    rampart_db::users::set_totp_secret(state.pool(), user.0.id, &secret).await?;
    Ok(Json(SetupResp {
        secret,
        otpauth_uri: uri,
    }))
}

#[derive(Deserialize)]
struct EnableInput {
    code: String,
}

#[derive(Serialize)]
struct EnableResp {
    enabled: bool,
    recovery_codes: Vec<String>,
}

async fn enable(
    State(state): State<AppState>,
    user: AuthUser,
    Json(input): Json<EnableInput>,
) -> Result<Json<EnableResp>, ApiError> {
    // Re-fetch the user row to see the not-yet-active secret.
    let raw = rampart_db::users::get_by_email(state.pool(), &user.0.email)
        .await
        .map_err(|_| ApiError::Unauthorized)?;
    let secret = raw
        .totp_secret
        .ok_or_else(|| ApiError::BadRequest("call /setup first".into()))?;

    if !totp::verify(&secret, &input.code) {
        return Err(ApiError::BadRequest("invalid code".into()));
    }

    rampart_db::users::enable_totp(state.pool(), user.0.id).await?;
    let codes = state.store().issue_recovery_codes(user.0.id, 10).await?;
    Ok(Json(EnableResp {
        enabled: true,
        recovery_codes: codes,
    }))
}

#[derive(Deserialize)]
struct DisableInput {
    password: String,
    code: String,
}

async fn disable(
    State(state): State<AppState>,
    user: AuthUser,
    Json(input): Json<DisableInput>,
) -> Result<StatusCode, ApiError> {
    let raw = rampart_db::users::get_by_email(state.pool(), &user.0.email)
        .await
        .map_err(|_| ApiError::Unauthorized)?;
    if !verify_password(&input.password, &raw.password_hash) {
        return Err(ApiError::Unauthorized);
    }
    // Code can be either a TOTP or a recovery code — either proves
    // the user holds the second factor or has the printed escape hatch.
    let code_ok = raw
        .totp_secret
        .as_deref()
        .map(|s| totp::verify(s, &input.code))
        .unwrap_or(false)
        || state
            .store()
            .consume_recovery_code(user.0.id, &input.code)
            .await?;

    if !code_ok {
        return Err(ApiError::BadRequest("invalid code".into()));
    }
    rampart_db::users::disable_totp(state.pool(), user.0.id).await?;
    state
        .store()
        .delete_recovery_codes_for_user(user.0.id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct VerifyInput {
    challenge_token: String,
    code: String,
}

#[derive(Serialize)]
struct VerifyResp {
    user: User,
}

async fn verify(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<VerifyInput>,
) -> Result<axum::response::Response, ApiError> {
    let token = Uuid::from_str(&input.challenge_token).map_err(|_| ApiError::Unauthorized)?;
    let user_id = state
        .consume_totp_challenge(token)
        .await
        .ok_or(ApiError::Unauthorized)?;

    let raw = rampart_db::users::get(state.pool(), user_id).await?;

    // Brute-force gate: the verify step re-issues a fresh challenge on every
    // wrong code, so without a durable cap a caller past the password gate could
    // grind the 6-digit TOTP / recovery codes (MFA bypass). After
    // TOTP_MAX_ATTEMPTS consecutive failures the account is locked out of the
    // 2FA step for TOTP_LOCKOUT_MINS; refuse immediately and withhold a fresh
    // challenge so the loop can't continue without a new (rate-limited) password
    // round-trip. A successful verify clears the counter.
    if let Some(until) = rampart_db::users::totp_locked_until(state.pool(), user_id).await? {
        if until > time::OffsetDateTime::now_utc() {
            return Ok((
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({
                    "error": "too_many_attempts",
                })),
            )
                .into_response());
        }
    }

    // Re-load secret since `get` strips it.
    let with_hash = rampart_db::users::get_by_email(state.pool(), &raw.email).await?;

    let code_ok = with_hash
        .totp_secret
        .as_deref()
        .map(|s| totp::verify(s, &input.code))
        .unwrap_or(false)
        || state
            .store()
            .consume_recovery_code(user_id, &input.code)
            .await?;

    if !code_ok {
        // Security event: password was correct but the second factor
        // failed — identity is known (the password gate passed), so
        // attribute it to the user.
        crate::audit::record(
            state.pool(),
            &raw,
            &headers,
            "auth.totp_failed",
            "session",
            None,
            None,
        )
        .await;
        let locked = rampart_db::users::record_totp_failure(
            state.pool(),
            user_id,
            TOTP_MAX_ATTEMPTS,
            TOTP_LOCKOUT_MINS,
        )
        .await?;
        if locked {
            // Hit the cap — refuse and DON'T re-issue a challenge.
            return Ok((
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({
                    "error": "too_many_attempts",
                })),
            )
                .into_response());
        }
        // Re-issue a fresh challenge so retries don't require a full
        // password round-trip. Bound by the same 5-minute window.
        let next = state.issue_totp_challenge(user_id).await;
        return Ok((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error":           "invalid_code",
                "challenge_token": next.to_string(),
            })),
        )
            .into_response());
    }

    // Success — clear the brute-force counter.
    rampart_db::users::reset_totp_failures(state.pool(), user_id)
        .await
        .ok();

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
        Json(VerifyResp { user }),
    )
        .into_response())
}
