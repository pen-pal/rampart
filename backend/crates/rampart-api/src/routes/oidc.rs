//! OpenID Connect (SSO) login — Authorization Code flow with PKCE.
//!
//! Lets an operator put Rampart behind their IdP (Google, Okta, Keycloak,
//! Authentik, Entra, …) instead of local password accounts. Generic OIDC: it
//! reads the provider's discovery document and uses the **userinfo** endpoint
//! for identity, so there is no JWT-signature verification to get wrong — the
//! access token is exchanged server-to-server over TLS with the trusted issuer.
//!
//! Configured entirely via env (enabled when all four are set):
//!   RAMPART_OIDC_ISSUER         e.g. https://accounts.google.com
//!   RAMPART_OIDC_CLIENT_ID
//!   RAMPART_OIDC_CLIENT_SECRET
//!   RAMPART_OIDC_REDIRECT_URL   e.g. https://rampart.example.com/v1/auth/oidc/callback
//!   RAMPART_OIDC_DEFAULT_ROLE   admin|editor|readonly (default readonly; the
//!                               very first user provisioned becomes admin)
//!   RAMPART_OIDC_ORG_CLAIM      optional userinfo claim (e.g. `groups`, a
//!                               custom `org`, or Google's `hd`) mapping the
//!                               identity to org(s) BY SLUG — each value is
//!                               slug-normalised, matched to an existing org,
//!                               granted membership (at DEFAULT_ROLE), and the
//!                               first match becomes the session's active org.
//!                               Unset ⇒ no org mapping (Default org as before).
//!
//! Routes (all public, mounted at /v1/auth/oidc):
//!   GET /config    → { enabled } so the login page can show an SSO button
//!   GET /login     → 302 to the IdP (state + PKCE stashed server-side)
//!   GET /callback  → exchange code → userinfo → provision user → session

use crate::auth::{build_session_cookie, is_secure};
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{AppendHeaders, IntoResponse, Redirect};
use axum::routing::get;
use axum::{Json, Router};
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;
use base64::Engine;
use rampart_core::Role;
use rampart_db::users::NewUser;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const SESSION_TTL_SECS: i64 = 60 * 60 * 24 * 7;
const STATE_TTL: Duration = Duration::from_secs(600);

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/config", get(config_endpoint))
        .route("/login", get(login))
        .route("/callback", get(callback))
}

// ── env config ────────────────────────────────────────────────────────────

struct OidcConfig {
    issuer: String,
    client_id: String,
    client_secret: String,
    redirect_url: String,
    default_role: Role,
    /// Optional userinfo claim that maps the identity to org(s) by slug
    /// (Phase 4f). `None` ⇒ no org mapping — behaviour identical to pre-4f
    /// (provision into the Default org, active org unset).
    org_claim: Option<String>,
}

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|s| !s.trim().is_empty())
}

fn config() -> Option<OidcConfig> {
    Some(OidcConfig {
        issuer: env("RAMPART_OIDC_ISSUER")?
            .trim_end_matches('/')
            .to_string(),
        client_id: env("RAMPART_OIDC_CLIENT_ID")?,
        client_secret: env("RAMPART_OIDC_CLIENT_SECRET")?,
        redirect_url: env("RAMPART_OIDC_REDIRECT_URL")?,
        default_role: match env("RAMPART_OIDC_DEFAULT_ROLE").as_deref() {
            Some("admin") => Role::Admin,
            Some("editor") => Role::Editor,
            _ => Role::Readonly,
        },
        org_claim: env("RAMPART_OIDC_ORG_CLAIM"),
    })
}

// ── pending-state store (state → PKCE verifier) ─────────────────────────────

struct Pending {
    verifier: String,
    created: Instant,
}

fn state_store() -> &'static Mutex<HashMap<String, Pending>> {
    static STORE: std::sync::OnceLock<Mutex<HashMap<String, Pending>>> = std::sync::OnceLock::new();
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn stash(state: String, verifier: String) {
    let mut g = state_store().lock().unwrap();
    let now = Instant::now();
    g.retain(|_, p| now.duration_since(p.created) < STATE_TTL); // opportunistic GC
    g.insert(
        state,
        Pending {
            verifier,
            created: now,
        },
    );
}

fn take(state: &str) -> Option<String> {
    let mut g = state_store().lock().unwrap();
    let p = g.remove(state)?;
    (Instant::now().duration_since(p.created) < STATE_TTL).then_some(p.verifier)
}

// ── discovery + token/userinfo wire types ───────────────────────────────────

#[derive(Deserialize)]
struct Discovery {
    authorization_endpoint: String,
    token_endpoint: String,
    userinfo_endpoint: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
}

#[derive(Deserialize)]
struct UserInfo {
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    name: Option<String>,
    /// The provider's assertion that it has verified ownership of `email`.
    /// We refuse to provision/link a Rampart account unless this is `true` —
    /// otherwise anyone who can register an *unverified* account at the IdP
    /// with a victim's address could take over the matching Rampart user.
    #[serde(default, deserialize_with = "de_bool_lenient")]
    email_verified: Option<bool>,
    /// All other userinfo claims, captured so the configurable org-mapping
    /// (Phase 4f) can read whichever claim `RAMPART_OIDC_ORG_CLAIM` names
    /// (e.g. `groups`, a custom `org`, or Google's hosted-domain `hd`).
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

/// Slugs (org slugs) a userinfo claim maps to. The claim value may be a single
/// string (`hd`, a custom `org`) or an array of strings (`groups`); each value
/// is normalised to slug form (lowercase, non-`[a-z0-9]` runs → `-`) so e.g.
/// `"Acme Corp"`→`acme-corp` and `"acme.com"`→`acme-com` match an org by slug.
/// Empty when the claim is absent/of an unexpected shape.
fn claim_org_slugs(extra: &serde_json::Map<String, serde_json::Value>, claim: &str) -> Vec<String> {
    let mut out = Vec::new();
    match extra.get(claim) {
        Some(serde_json::Value::String(s)) => out.push(normalize_slug(s)),
        Some(serde_json::Value::Array(arr)) => {
            for x in arr {
                if let Some(s) = x.as_str() {
                    out.push(normalize_slug(s));
                }
            }
        }
        _ => {}
    }
    out.retain(|s| s.len() >= 2);
    out
}

/// Lowercase + collapse any run of non-alphanumeric chars to a single `-`,
/// trimming leading/trailing `-`. Mirrors the org-slug charset (`[a-z0-9-]`).
fn normalize_slug(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_dash = false;
    for c in s.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            prev_dash = false;
        } else if !out.is_empty() && !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// Some IdPs encode `email_verified` as a JSON bool, others as the strings
/// "true"/"false". Accept either; absent/null/other → `None` (unverified).
fn de_bool_lenient<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    use serde::Deserialize;
    Ok(match Option::<serde_json::Value>::deserialize(d)? {
        Some(serde_json::Value::Bool(b)) => Some(b),
        Some(serde_json::Value::String(s)) => Some(s.eq_ignore_ascii_case("true")),
        _ => None,
    })
}

async fn discover(client: &reqwest::Client, issuer: &str) -> Result<Discovery, ApiError> {
    let url = format!("{issuer}/.well-known/openid-configuration");
    client
        .get(url)
        .send()
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("oidc discovery: {e}")))?
        .json::<Discovery>()
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("oidc discovery parse: {e}")))
}

fn rand_token() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    (0..48)
        .map(|_| {
            const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
            A[rng.gen_range(0..A.len())] as char
        })
        .collect()
}

// ── handlers ────────────────────────────────────────────────────────────────

async fn config_endpoint() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "enabled": config().is_some() }))
}

async fn login() -> Result<impl IntoResponse, ApiError> {
    let cfg = config().ok_or(ApiError::NotFound)?;
    let client = reqwest::Client::new();
    let disco = discover(&client, &cfg.issuer).await?;

    let verifier = rand_token();
    let challenge = B64URL.encode(Sha256::digest(verifier.as_bytes()));
    let state = rand_token();
    stash(state.clone(), verifier);

    let auth_url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256",
        disco.authorization_endpoint,
        urlencoding(&cfg.client_id),
        urlencoding(&cfg.redirect_url),
        urlencoding("openid email profile"),
        urlencoding(&state),
        urlencoding(&challenge),
    );
    Ok(Redirect::temporary(&auth_url))
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

async fn callback(
    State(app): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<CallbackQuery>,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(err) = q.error {
        return Err(ApiError::BadRequest(format!("oidc provider error: {err}")));
    }
    let cfg = config().ok_or(ApiError::NotFound)?;
    let code = q.code.ok_or(ApiError::BadRequest("missing code".into()))?;
    let state = q
        .state
        .ok_or(ApiError::BadRequest("missing state".into()))?;
    let verifier = take(&state).ok_or(ApiError::Unauthorized)?; // unknown/expired state

    let client = reqwest::Client::new();
    let disco = discover(&client, &cfg.issuer).await?;

    // Exchange the code for an access token (PKCE + confidential client).
    let token: TokenResponse = client
        .post(&disco.token_endpoint)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", &cfg.redirect_url),
            ("client_id", &cfg.client_id),
            ("client_secret", &cfg.client_secret),
            ("code_verifier", &verifier),
        ])
        .send()
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("oidc token: {e}")))?
        .json()
        .await
        .map_err(|_| ApiError::Unauthorized)?;

    // Identity via userinfo (server-to-server, TLS-trusted — no JWT crypto).
    let info: UserInfo = client
        .get(&disco.userinfo_endpoint)
        .bearer_auth(&token.access_token)
        .send()
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("oidc userinfo: {e}")))?
        .json()
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("oidc userinfo parse: {e}")))?;

    // Refuse to trust an unverified email — otherwise an attacker who registers
    // an unverified account at the IdP under a victim's address could log in as
    // (or provision) that victim's Rampart user.
    if info.email_verified != Some(true) {
        return Err(ApiError::BadRequest(
            "oidc: provider did not assert a verified email (email_verified=true); \
             refusing to provision or link an account"
                .into(),
        ));
    }

    // Phase 4f: resolve the configured org-mapping claim now, before `info` is
    // consumed below. Empty (and a no-op) unless RAMPART_OIDC_ORG_CLAIM is set.
    let org_slugs: Vec<String> = cfg
        .org_claim
        .as_deref()
        .map(|c| claim_org_slugs(&info.extra, c))
        .unwrap_or_default();

    let email = info
        .email
        .map(|e| e.to_lowercase())
        .filter(|e| e.contains('@'))
        .ok_or(ApiError::BadRequest("oidc: no email in userinfo".into()))?;

    // Find or provision the user.
    let user = match rampart_db::users::get_by_email(app.pool(), &email).await {
        Ok(u) => rampart_db::users::get(app.pool(), u.id).await?,
        Err(_) => {
            // First user bootstraps as admin; otherwise the configured role.
            let role = if rampart_db::users::count(app.pool()).await? == 0 {
                Role::Admin
            } else {
                cfg.default_role
            };
            rampart_db::users::create(
                app.pool(),
                NewUser {
                    email: email.clone(),
                    name: Some(info.name.unwrap_or_else(|| email.clone())),
                    // No password login for SSO users: store an unusable random hash.
                    password_hash: crate::auth::hash_password(&rand_token())?,
                    role,
                },
            )
            .await?
        }
    };

    // Phase 4f: grant membership in each mapped org (matched by slug) and pick
    // the first match as the active org. Unmatched slugs are ignored (no
    // auto-create, no deny) — the user falls back to Default. Idempotent, so
    // memberships re-sync on every login. No-op when org_slugs is empty.
    let mut mapped_org: Option<rampart_core::ids::OrgId> = None;
    for slug in &org_slugs {
        if let Ok(org) = rampart_db::orgs::get_by_slug(app.pool(), slug).await {
            rampart_db::orgs::upsert_member(app.pool(), org.id, user.id, cfg.default_role).await?;
            if mapped_org.is_none() {
                mapped_org = Some(org.id);
            }
        }
    }

    rampart_db::users::mark_login(app.pool(), user.id)
        .await
        .ok();

    let session = rampart_db::sessions::create(
        app.pool(),
        user.id,
        SESSION_TTL_SECS,
        None,
        headers
            .get("user-agent")
            .and_then(|v| v.to_str().ok())
            .map(String::from),
    )
    .await?;

    // Phase 4f: point the new session at the first mapped org (Phase 4e then
    // scopes the user's requests there with their per-org role). Best-effort.
    if let Some(org) = mapped_org {
        rampart_db::sessions::set_active_org(app.pool(), session.id, user.id, org.0)
            .await
            .ok();
    }

    let cookie = build_session_cookie(session.id, is_secure(&headers));
    Ok((
        StatusCode::SEE_OTHER,
        AppendHeaders([
            (header::SET_COOKIE, cookie.to_string()),
            (header::LOCATION, "/".to_string()),
        ]),
    ))
}

/// Minimal percent-encoding for query components (alnum + unreserved pass
/// through; everything else is %XX). Avoids pulling a urlencoding crate.
fn urlencoding(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_is_b64url_sha256() {
        // RFC 7636 test vector.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = B64URL.encode(Sha256::digest(verifier.as_bytes()));
        assert_eq!(challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn urlencoding_escapes_reserved() {
        assert_eq!(
            urlencoding("openid email profile"),
            "openid%20email%20profile"
        );
        assert_eq!(urlencoding("a+b/c"), "a%2Bb%2Fc");
        assert_eq!(urlencoding("safe-_.~"), "safe-_.~");
    }

    #[test]
    fn state_store_roundtrip_and_single_use() {
        stash("st1".into(), "ver1".into());
        assert_eq!(take("st1").as_deref(), Some("ver1"));
        assert_eq!(take("st1"), None); // consumed
    }

    // ── Phase 4f: org-claim → slug resolution ────────────────────────────────
    fn obj(v: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn normalize_slug_cases() {
        assert_eq!(normalize_slug("Acme Corp"), "acme-corp");
        assert_eq!(normalize_slug("acme.com"), "acme-com");
        assert_eq!(normalize_slug("  Already-Slug  "), "already-slug");
        assert_eq!(normalize_slug("a/b__c"), "a-b-c");
        assert_eq!(normalize_slug("--x--"), "x");
    }

    #[test]
    fn claim_string_array_and_missing() {
        // single-string claim (hd / custom org)
        assert_eq!(
            claim_org_slugs(&obj(serde_json::json!({"hd": "Acme.com"})), "hd"),
            vec!["acme-com"]
        );
        // array claim (groups): non-strings ignored, sub-2-char dropped
        assert_eq!(
            claim_org_slugs(
                &obj(serde_json::json!({"groups": ["Acme Corp", "beta", 42, "x"]})),
                "groups"
            ),
            vec!["acme-corp", "beta"]
        );
        // absent claim → empty (no-op mapping)
        assert!(claim_org_slugs(&obj(serde_json::json!({"groups": []})), "nope").is_empty());
        // unexpected shape → empty
        assert!(
            claim_org_slugs(&obj(serde_json::json!({"org": {"k": true}})), "org").is_empty()
        );
    }
}
