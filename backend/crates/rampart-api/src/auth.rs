//! Authentication primitives.
//!
//! Password hashing (argon2), session token helpers, the `AuthUser`
//! extractor that pulls + validates the session cookie, and the tower
//! middleware that gates protected routes.
//!
//! Sessions are server-side. The cookie's only value is the session id —
//! a v4 UUID (cryptographically random). The DB row carries the user_id,
//! expiry, and audit fields.

use crate::error::ApiError;
use crate::state::AppState;
use argon2::password_hash::{rand_core::OsRng, SaltString};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use axum::extract::{FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::Response;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use rampart_db::users::User;
use std::str::FromStr;
use time::Duration;
use uuid::Uuid;

pub const SESSION_COOKIE: &str = "rampart_session";
pub const SESSION_TTL_SECS: i64 = 60 * 60 * 24 * 14; // 14 days

/// Hash a plaintext password with Argon2id and per-password random salt.
pub fn hash_password(plaintext: &str) -> Result<String, ApiError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(plaintext.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| ApiError::BadRequest(format!("could not hash password: {e}")))
}

/// Verify a plaintext password against a previously-hashed string. Never
/// short-circuits on length so we don't leak timing info.
pub fn verify_password(plaintext: &str, hashed: &str) -> bool {
    PasswordHash::new(hashed)
        .and_then(|h| Argon2::default().verify_password(plaintext.as_bytes(), &h))
        .is_ok()
}

/// Build the Set-Cookie for a freshly-created session. `Secure` is set
/// when the deployment terminates TLS (we look for a forwarding proxy
/// header or assume true in production). For local dev we keep it
/// permissive.
pub fn build_session_cookie<'a>(token: Uuid, secure: bool) -> Cookie<'a> {
    let mut c = Cookie::new(SESSION_COOKIE, token.to_string());
    c.set_http_only(true);
    c.set_same_site(SameSite::Strict);
    c.set_secure(secure);
    c.set_path("/");
    c.set_max_age(Duration::seconds(SESSION_TTL_SECS));
    c
}

/// A "clear cookie" for logout. Same name and path, expired immediately.
pub fn build_clear_cookie<'a>(secure: bool) -> Cookie<'a> {
    let mut c = Cookie::new(SESSION_COOKIE, "");
    c.set_http_only(true);
    c.set_same_site(SameSite::Strict);
    c.set_secure(secure);
    c.set_path("/");
    c.set_max_age(Duration::seconds(0));
    c
}

/// Extractor for endpoints that require an authenticated user. Looks the
/// session up on every request — cheap enough at our scale, and avoids
/// stale-state bugs if we revoke a session out-of-band.
pub struct AuthUser(pub User);

#[axum::async_trait]
impl FromRequestParts<AppState> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_headers(&parts.headers);
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

        Ok(AuthUser(user))
    }
}

/// Middleware version of the extractor — for protecting whole subtrees
/// without each handler having to declare `AuthUser` in its signature.
/// On success, attaches the `User` to the request extensions.
///
/// Accepts either:
/// - `rampart_session` HttpOnly cookie (browser flow), OR
/// - `Authorization: Bearer rmp_…` API key (script/automation flow).
///
/// API keys hit a different lookup but resolve to the same `User`, so
/// downstream handlers don't need to care which path was used.
pub async fn require_session(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    // Try bearer first — cheap header read, no DB hit if absent.
    if let Some(token) = bearer_token(req.headers()) {
        let (key, user_id) = rampart_db::api_keys::lookup(state.pool(), &token)
            .await
            .map_err(|_| ApiError::Unauthorized)?;
        // Fire-and-forget — don't block the request on the bump.
        let pool = state.pool().clone();
        let key_id = key.id;
        tokio::spawn(async move {
            let _ = rampart_db::api_keys::touch_last_used(&pool, key_id).await;
        });

        let user = rampart_db::users::get(state.pool(), user_id)
            .await
            .map_err(|_| ApiError::Unauthorized)?;
        req.extensions_mut().insert(user);
        return Ok(next.run(req).await);
    }

    let jar = CookieJar::from_headers(req.headers());
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

    req.extensions_mut().insert(user);
    Ok(next.run(req).await)
}

/// Extract a bearer token from the Authorization header. Trims whitespace
/// and accepts `Bearer` case-insensitively (some clients lowercase it).
fn bearer_token(headers: &axum::http::HeaderMap) -> Option<String> {
    let v = headers.get(axum::http::header::AUTHORIZATION)?.to_str().ok()?;
    let mut parts = v.splitn(2, ' ');
    let scheme = parts.next()?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = parts.next()?.trim();
    if token.is_empty() {
        return None;
    }
    Some(token.to_string())
}

/// Like `require_session` but additionally rejects non-admins with 403.
/// Apply on top of `require_session` (it relies on the User in extensions).
pub async fn require_admin(
    req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let user = req
        .extensions()
        .get::<rampart_db::users::User>()
        .ok_or(ApiError::Unauthorized)?
        .clone();
    if !user.is_admin {
        return Err(ApiError::Forbidden);
    }
    Ok(next.run(req).await)
}

/// Should Set-Cookie include `Secure`? True if the request was forwarded
/// over HTTPS (proxy sets `X-Forwarded-Proto: https`) — otherwise local
/// dev on plain http breaks.
pub fn is_secure(headers: &axum::http::HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.eq_ignore_ascii_case("https"))
        .unwrap_or(false)
}
