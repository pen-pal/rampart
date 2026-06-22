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

/// Request-extension marker carrying the id of the API key that
/// authenticated the request, plus that key's configured per-hour budget.
/// Inserted by `require_session` only on the bearer / api-key path — cookie
/// sessions never set it. The per-key rate-limit layer in `lib.rs` reads it
/// to (a) scope the rolling-hour counter to the key, (b) cap the window at
/// the key's own `rate_limit_per_hour` budget, and (c) decide whether to
/// emit `X-RateLimit-*` headers at all (absent → session request →
/// unlimited, no headers).
#[derive(Debug, Clone, Copy)]
pub struct AuthApiKeyId {
    pub id: rampart_core::ApiKeyId,
    /// The key's persisted per-hour request budget (migration 0067).
    pub rate_limit_per_hour: i32,
}

/// Request-extension carrying the org the request is acting in (multi-tenancy)
/// and the caller's role within that org. Inserted by `require_session` on
/// every authenticated request alongside the `User`.
///
/// Phase 1 is forward-compat plumbing only: the cookie path resolves the org
/// from the session's `active_org_id` (falling back to the Default org), the
/// bearer/api-key path resolves to the Default org (keys don't carry an org
/// yet), and `role` mirrors the user's global role. No query filters by org
/// and the RBAC guards still read `User.role`, so behaviour is unchanged —
/// later phases switch reads + guards to consult this context.
#[derive(Debug, Clone, Copy)]
pub struct OrgContext {
    pub org_id: rampart_core::ids::OrgId,
    pub role: rampart_core::Role,
}

/// Hash a plaintext password with Argon2id and per-password random salt.
pub fn hash_password(plaintext: &str) -> Result<String, ApiError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(plaintext.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| ApiError::BadRequest(format!("could not hash password: {e}")))
}

/// A small set of obviously-weak passwords to reject outright (lowercased).
/// Not a full HIBP corpus — just enough to stop the embarrassing ones.
const WEAK_PASSWORDS: &[&str] = &[
    "password",
    "password1",
    "password123",
    "passw0rd",
    "1234567890",
    "12345678",
    "123456789",
    "qwerty123",
    "letmein123",
    "changeme123",
    "admin12345",
    "welcome123",
    "iloveyou123",
    "rampart123",
];

/// Validate a new password against the account policy. `Err(reason)` on failure:
/// ≥10 chars, not a known-weak password, not derived from the email's local
/// part, and not a single repeated character. Dependency-free; used by register,
/// admin user-create, and self-service password change so the rule is uniform.
pub fn validate_password(pw: &str, email: &str) -> Result<(), ApiError> {
    let bad = |m: &str| Err(ApiError::BadRequest(m.to_string()));
    if pw.chars().count() < 10 {
        return bad("password must be at least 10 characters");
    }
    let lower = pw.to_lowercase();
    if WEAK_PASSWORDS.contains(&lower.as_str()) {
        return bad("password is too common — choose something less guessable");
    }
    if let Some(local) = email.split('@').next() {
        let local = local.to_lowercase();
        if local.len() >= 3 && lower.contains(&local) {
            return bad("password must not contain your email name");
        }
    }
    if pw
        .chars()
        .collect::<std::collections::BTreeSet<char>>()
        .len()
        <= 1
    {
        return bad("password must not be a single repeated character");
    }
    Ok(())
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

// axum 0.8 dropped the `#[async_trait]` requirement on extractor
// implementations — native async-fn-in-trait carries the signature
// directly since rustc 1.75.
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

        let session = state
            .store()
            .lookup_session(token)
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
        let (key, user_id, key_org) = state
            .store()
            .lookup_api_key(&token)
            .await
            .map_err(|_| ApiError::Unauthorized)?;
        // Fire-and-forget — don't block the request on the bump.
        let pool = state.pool().clone();
        let key_id = key.id;
        tokio::spawn(async move {
            let _ = rampart_db::api_keys::touch_last_used(&pool, key_id).await;
        });

        let mut user = rampart_db::users::get(state.pool(), user_id)
            .await
            .map_err(|_| ApiError::Unauthorized)?;
        // Per-key authorization (migration 0057): the request's effective
        // role comes from the KEY's scope, NOT the creator's user role. This
        // is what enforces scopes — a `read` key gets Role::Readonly even if
        // an admin minted it, so the existing RBAC route guards (which read
        // `user.role`) 403 it on mutations / admin routes automatically.
        // `min(creator_role, key_scope_role)` would be stricter, but scopes
        // are the contract here, so the key's scope is authoritative.
        user.role = key.scope.as_role();
        // Org context: the request is scoped to the KEY's own owning org
        // (Phase 6 — keys are pinned to their minting org). Single-org installs
        // mint every key in the Default org, so this stays Default there. Role
        // mirrors the (key-scoped) user role.
        let org_ctx = OrgContext {
            org_id: key_org,
            role: user.role,
        };
        req.extensions_mut().insert(user);
        req.extensions_mut().insert(org_ctx);
        // Stash the authenticating key id so a downstream layer (the
        // per-key rate limiter in `lib.rs`) can identify which key made
        // the request. Only set on the api-key path — cookie/session
        // requests never carry it, which is exactly how the rate limiter
        // tells the two apart (session requests stay unlimited).
        req.extensions_mut().insert(AuthApiKeyId {
            id: key.id,
            rate_limit_per_hour: key.rate_limit_per_hour,
        });
        // RLS: bind the request's org so the pool's before_acquire hook scopes
        // every DB hit downstream to this tenant (no-op when RAMPART_RLS off —
        // the hooks aren't installed). Plumbing only here; policies are dormant.
        return Ok(rampart_db::rls::CURRENT_ORG
            .scope(Some(org_ctx.org_id.0), next.run(req))
            .await);
    }

    let jar = CookieJar::from_headers(req.headers());
    let token = jar
        .get(SESSION_COOKIE)
        .and_then(|c| Uuid::from_str(c.value()).ok())
        .ok_or(ApiError::Unauthorized)?;

    let session = state
        .store()
        .lookup_session(token)
        .await
        .map_err(|_| ApiError::Unauthorized)?;
    let mut user = rampart_db::users::get(state.pool(), session.user_id)
        .await
        .map_err(|_| ApiError::Unauthorized)?;

    // Org context (Phase 4e — per-org RBAC): scope to the session's active org
    // and use the caller's role IN THAT ORG (from org_members), not their global
    // role. Fall back to the Default org when active_org is unset OR when the
    // user is no longer a member of it (membership revoked mid-session — never
    // lock them out). `user.role` is overwritten with the per-org effective role
    // so the existing RBAC guards (which read `user.role`) enforce per-org
    // automatically; `user.is_admin` stays the GLOBAL flag (the 2FA-enforcement
    // policy and global-admin surfaces key off it). Single-org behaviour is
    // identical: active_org is unset → Default, and member_role(Default) mirrors
    // users.role (maintained by users::set_role/set_admin + create seeding).
    let default_org = rampart_core::ids::OrgId::from_uuid(rampart_core::org::DEFAULT_ORG_ID);
    let want = session
        .active_org_id
        .map(rampart_core::ids::OrgId::from_uuid)
        .unwrap_or(default_org);
    let (org_id, role) = match rampart_db::orgs::member_role(state.pool(), want, user.id).await {
        Ok(Some(r)) => (want, r),
        _ => {
            // Not a member of the active org (revoked / stale) or lookup failed:
            // fall back to the Default org + the caller's Default-org role.
            let r = rampart_db::orgs::member_role(state.pool(), default_org, user.id)
                .await
                .ok()
                .flatten()
                .unwrap_or(user.role);
            (default_org, r)
        }
    };
    user.role = role;
    let org_ctx = OrgContext { org_id, role };
    req.extensions_mut().insert(user);
    req.extensions_mut().insert(org_ctx);
    // RLS: bind the session's org for the downstream handler chain (no-op when
    // RAMPART_RLS off — hooks not installed). Plumbing only; policies dormant.
    Ok(rampart_db::rls::CURRENT_ORG
        .scope(Some(org_ctx.org_id.0), next.run(req))
        .await)
}

/// Extract a bearer token from the Authorization header. Trims whitespace
/// and accepts `Bearer` case-insensitively (some clients lowercase it).
fn bearer_token(headers: &axum::http::HeaderMap) -> Option<String> {
    let v = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
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
/// `role` is authoritative.
pub async fn require_admin(req: Request, next: Next) -> Result<Response, ApiError> {
    let user = req
        .extensions()
        .get::<rampart_db::users::User>()
        .ok_or(ApiError::Unauthorized)?
        .clone();
    if !user.role.is_admin() {
        return Err(ApiError::Forbidden);
    }
    Ok(next.run(req).await)
}

/// Rejects users who cannot write (readonly) with 403, but lets admin +
/// editor through regardless of HTTP verb. Apply on top of `require_session`
/// to gate route groups that editors are allowed to fully manage.
pub async fn require_editor(req: Request, next: Next) -> Result<Response, ApiError> {
    let user = req
        .extensions()
        .get::<rampart_db::users::User>()
        .ok_or(ApiError::Unauthorized)?
        .clone();
    if !user.role.can_write() {
        return Err(ApiError::Forbidden);
    }
    Ok(next.run(req).await)
}

/// Method-aware guard for the whole protected tree. Read-only verbs
/// (GET / HEAD / OPTIONS) are always allowed; any mutating verb requires
/// `can_write()` (admin or editor). This is what makes a `readonly` user
/// able to view everything but 403 on every POST/PUT/PATCH/DELETE.
///
/// Layered beneath the admin-only subtrees: those additionally apply
/// `require_admin`, so a non-admin editor still gets 403 there.
pub async fn require_write_or_readonly_get(req: Request, next: Next) -> Result<Response, ApiError> {
    use axum::http::Method;
    let read_only = matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    if read_only {
        return Ok(next.run(req).await);
    }
    let user = req
        .extensions()
        .get::<rampart_db::users::User>()
        .ok_or(ApiError::Unauthorized)?
        .clone();
    if !user.role.can_write() {
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

#[cfg(test)]
mod password_tests {
    use super::validate_password;

    #[test]
    fn password_policy() {
        // Good.
        assert!(validate_password("correct-horse-battery", "alice@example.com").is_ok());
        // Too short.
        assert!(validate_password("short", "a@b.com").is_err());
        // Common.
        assert!(validate_password("password123", "a@b.com").is_err());
        // Contains email local part.
        assert!(validate_password("alice-rampart-2026", "alice@example.com").is_err());
        // Single repeated char.
        assert!(validate_password("aaaaaaaaaaaa", "a@b.com").is_err());
    }
}
