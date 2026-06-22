//! OpenID Connect (SSO) login — Authorization Code flow with PKCE.
//!
//! Lets an operator put Rampart behind their IdP (Google, Okta, Keycloak,
//! Authentik, Entra, …) instead of local password accounts. Generic OIDC: it
//! reads the provider's discovery document, exchanges the code with PKCE, and
//! **cryptographically verifies the `id_token`** against the issuer's JWKS
//! (signature + `iss`/`aud`/`exp`/`nbf` + a `nonce` bound to the login attempt).
//! Identity is taken from the verified id_token claims, falling back to the
//! userinfo endpoint only for any claim the id_token omits.
//!
//! Hardening (issue #13):
//!   * Login state (CSRF `state` + PKCE verifier + `nonce`) lives in Postgres
//!     (`oidc_login_state`), one-time-use under a row lock, with a ~10-min TTL —
//!     so the flow survives restarts / multiple nodes and resists replay.
//!   * id_token signature is verified via the issuer JWKS. The header `alg` is
//!     allow-listed to ASYMMETRIC algorithms only (RS*/PS*/ES*/EdDSA) and a
//!     symmetric (`oct`/HMAC) JWK is rejected — defeating alg-confusion both on
//!     the header side and the key side. `nonce` is compared constant-time.
//!   * All outbound OIDC requests go through the SSRF-guarded HTTP client. The
//!     issuer's host is allow-listed for the private-range block so a private
//!     self-hosted IdP (Keycloak/Authentik on 10.x/192.168.x) stays reachable
//!     even under `RAMPART_SSRF_BLOCK_PRIVATE`, while the always-blocked set
//!     (cloud metadata 169.254.169.254 / loopback / link-local) is enforced
//!     regardless. Discovery/token/userinfo/jwks URLs are preflighted too.
//!
//! Configured entirely via env (enabled when all four are set):
//!   RAMPART_OIDC_ISSUER         e.g. https://accounts.google.com
//!   RAMPART_OIDC_CLIENT_ID
//!   RAMPART_OIDC_CLIENT_SECRET
//!   RAMPART_OIDC_REDIRECT_URL   e.g. https://rampart.example.com/v1/auth/oidc/callback
//!   RAMPART_OIDC_DEFAULT_ROLE   admin|editor|readonly (default readonly; the
//!                               very first user provisioned becomes admin)
//!   RAMPART_OIDC_ORG_CLAIM      optional claim (e.g. `groups`, a custom `org`,
//!                               or Google's `hd`) mapping the identity to
//!                               org(s) BY SLUG — each value is slug-normalised,
//!                               matched to an existing org, granted membership
//!                               (at DEFAULT_ROLE), and the first match becomes
//!                               the session's active org. Read from the
//!                               id_token claims first, then userinfo. Unset ⇒
//!                               no org mapping (Default org as before).
//!
//! Routes (all public, mounted at /v1/auth/oidc):
//!   GET /config    → { enabled } so the login page can show an SSO button
//!   GET /login     → 302 to the IdP (state + PKCE + nonce stashed in Postgres)
//!   GET /callback  → exchange code → verify id_token → provision user → session

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
use jsonwebtoken::jwk::{AlgorithmParameters, Jwk, JwkSet};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use rampart_core::Role;
use rampart_db::users::NewUser;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const SESSION_TTL_SECS: i64 = 60 * 60 * 24 * 7;

/// Asymmetric signing algorithms we accept on the id_token header. HS* (HMAC)
/// and `none` are deliberately absent: accepting them would let an attacker
/// re-sign a forged token with the (public) JWKS material or with no key at all
/// — the classic OIDC alg-confusion attack. Membership is checked manually
/// before key selection; `Validation` itself is then pinned to the SINGLE
/// header alg (see [`validate_id_token`]).
const ASYM: &[Algorithm] = &[
    Algorithm::RS256,
    Algorithm::RS384,
    Algorithm::RS512,
    Algorithm::PS256,
    Algorithm::PS384,
    Algorithm::PS512,
    Algorithm::ES256,
    Algorithm::ES384,
    Algorithm::EdDSA,
];

/// Per-process JWKS cache TTL (1h). The set is refetched when stale, empty, or
/// when a token references an unknown `kid` (key rotation).
const JWKS_TTL: Duration = Duration::from_secs(3600);
/// Minimum spacing between JWKS refetches. Bounds the amplification a stream of
/// bogus-`kid` tokens could otherwise cause: inside the cooldown, an unknown
/// kid is rejected without a network fetch.
const JWKS_REFETCH_COOLDOWN: Duration = Duration::from_secs(60);

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
    /// Optional claim that maps the identity to org(s) by slug (Phase 4f).
    /// `None` ⇒ no org mapping — behaviour identical to pre-4f.
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

/// The host component of the configured issuer URL. Allow-listed for the SSRF
/// private-range block so a private self-hosted IdP works under block-private.
/// `None` if the issuer can't be parsed (then no host is allow-listed — the
/// guard treats the IdP like any other host).
fn issuer_host(cfg: &OidcConfig) -> Vec<String> {
    reqwest::Url::parse(&cfg.issuer)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .into_iter()
        .collect()
}

/// Build an SSRF-guarded HTTP client for OIDC outbound traffic. The issuer host
/// is allow-listed for the private-range block (private self-hosted IdP), but
/// the always-blocked set (metadata/loopback/link-local) is enforced regardless.
fn guarded_client(allow_hosts: Vec<String>) -> Result<reqwest::Client, ApiError> {
    rampart_ssrf::guarded_client_builder_allowing(allow_hosts)
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("oidc client build: {e}")))
}

/// Preflight a URL through the SSRF guard (catches literal-IP URLs the DNS
/// resolver hook never sees). A blocked URL → `BadRequest`.
async fn preflight(url: &str, allow_hosts: &[String]) -> Result<(), ApiError> {
    rampart_ssrf::guard_url_allowing(url, allow_hosts)
        .await
        .map_err(|e| ApiError::BadRequest(format!("oidc endpoint blocked: {e}")))
}

// ── discovery + token/userinfo + id_token wire types ────────────────────────

#[derive(Deserialize)]
struct Discovery {
    authorization_endpoint: String,
    token_endpoint: String,
    userinfo_endpoint: String,
    jwks_uri: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    #[allow(dead_code)]
    access_token: String,
    /// The OIDC id_token (a signed JWT). Required — callback refuses a token
    /// response without one.
    #[serde(default)]
    id_token: Option<String>,
}

/// Claims we read from the verified id_token. `nonce` is mandatory (checked
/// manually, constant-time). Identity claims are optional here because any one
/// of them may legitimately live only in userinfo (the fallback path).
#[derive(Deserialize)]
struct IdClaims {
    #[serde(default)]
    nonce: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_bool_lenient")]
    email_verified: Option<bool>,
    /// All other claims, so the configurable org-mapping can read whichever
    /// claim `RAMPART_OIDC_ORG_CLAIM` names directly from the id_token.
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct UserInfo {
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    name: Option<String>,
    /// The provider's assertion that it has verified ownership of `email`.
    /// We refuse to provision/link a Rampart account unless this is `true`.
    #[serde(default, deserialize_with = "de_bool_lenient")]
    email_verified: Option<bool>,
    /// All other userinfo claims, captured for the configurable org-mapping.
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

/// Slugs (org slugs) a claim maps to. The value may be a single string (`hd`, a
/// custom `org`) or an array of strings (`groups`); each value is normalised to
/// slug form (lowercase, non-`[a-z0-9]` runs → `-`). Empty when the claim is
/// absent/of an unexpected shape.
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

// ── JWKS cache ──────────────────────────────────────────────────────────────

struct CachedJwks {
    set: JwkSet,
    fetched: Instant,
    last_refetch: Instant,
}

fn jwks_cache() -> &'static Mutex<Option<CachedJwks>> {
    static CACHE: std::sync::OnceLock<Mutex<Option<CachedJwks>>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// Fetch and parse the JWKS from `jwks_uri` through the guarded client.
async fn fetch_jwks(
    client: &reqwest::Client,
    jwks_uri: &str,
    allow_hosts: &[String],
) -> Result<JwkSet, ApiError> {
    preflight(jwks_uri, allow_hosts).await?;
    client
        .get(jwks_uri)
        .send()
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("oidc jwks: {e}")))?
        .json::<JwkSet>()
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("oidc jwks parse: {e}")))
}

/// Resolve the JWK matching `kid` (or the sole key when the token carries no
/// `kid`), using the per-process cache. Refetches when the cache is empty/stale
/// or the kid is unknown — but at most once per [`JWKS_REFETCH_COOLDOWN`], so a
/// flood of bogus-kid tokens can't amplify into a flood of JWKS fetches.
async fn resolve_jwk(
    client: &reqwest::Client,
    jwks_uri: &str,
    allow_hosts: &[String],
    kid: Option<&str>,
) -> Result<Jwk, ApiError> {
    // First, try the cache without a network round-trip.
    {
        let guard = jwks_cache().lock().unwrap();
        if let Some(c) = guard.as_ref() {
            if c.fetched.elapsed() < JWKS_TTL {
                if let Some(jwk) = pick_jwk(&c.set, kid) {
                    return Ok(jwk.clone());
                }
                // Known cache, unknown kid → only refetch outside the cooldown.
                if c.last_refetch.elapsed() < JWKS_REFETCH_COOLDOWN {
                    return Err(ApiError::Unauthorized);
                }
            }
        }
    }

    // Cache miss / stale / unknown-kid-past-cooldown: refetch.
    let set = fetch_jwks(client, jwks_uri, allow_hosts).await?;
    let picked = pick_jwk(&set, kid).cloned();
    {
        let now = Instant::now();
        let mut guard = jwks_cache().lock().unwrap();
        *guard = Some(CachedJwks {
            set,
            fetched: now,
            last_refetch: now,
        });
    }
    picked.ok_or(ApiError::Unauthorized)
}

/// Select the JWK for `kid`, or the sole key when the token carries no kid.
fn pick_jwk<'a>(set: &'a JwkSet, kid: Option<&str>) -> Option<&'a Jwk> {
    match kid {
        Some(kid) => set.find(kid),
        None if set.keys.len() == 1 => Some(&set.keys[0]),
        None => None,
    }
}

/// Verify the id_token end-to-end and return the (trusted) claims.
///
/// Order matters for the alg-confusion defence:
///  1. header.alg must be in the ASYMMETRIC allow-list (rejects HS*/none);
///  2. the matched JWK must be asymmetric (RSA/EC/OKP) — a symmetric `oct`
///     (HMAC) JWK is rejected (key-side alg-confusion);
///  3. `Validation` is pinned to the SINGLE header alg — NOT the whole ASYM
///     list (a multi-family list rejects every token in jsonwebtoken v10).
async fn validate_id_token(
    client: &reqwest::Client,
    jwks_uri: &str,
    allow_hosts: &[String],
    id_token: &str,
    cfg: &OidcConfig,
    expected_nonce: &str,
) -> Result<IdClaims, ApiError> {
    let header = decode_header(id_token).map_err(|_| ApiError::Unauthorized)?;
    // (1) header alg allow-list — rejects HS*/none → defeats alg-confusion.
    if !ASYM.contains(&header.alg) {
        return Err(ApiError::Unauthorized);
    }

    let jwk = resolve_jwk(client, jwks_uri, allow_hosts, header.kid.as_deref()).await?;

    // (2) reject a symmetric (oct/HMAC) JWK — key-side alg-confusion defence.
    // Only RSA / EllipticCurve / OctetKeyPair (Ed) are asymmetric.
    match jwk.algorithm {
        AlgorithmParameters::RSA(_)
        | AlgorithmParameters::EllipticCurve(_)
        | AlgorithmParameters::OctetKeyPair(_) => {}
        AlgorithmParameters::OctetKey(_) => return Err(ApiError::Unauthorized),
    }

    let key = DecodingKey::from_jwk(&jwk).map_err(|_| ApiError::Unauthorized)?;

    // (3) single-alg Validation — pin to the header alg, never the full list.
    let mut v = Validation::new(header.alg);
    v.set_issuer(&[cfg.issuer.as_str()]);
    v.set_audience(&[cfg.client_id.as_str()]);
    v.set_required_spec_claims(&["exp", "iss", "aud"]);
    v.validate_nbf = true;
    // validate_exp / validate_aud default true; leeway 60 by default.

    let data =
        decode::<IdClaims>(id_token, &key, &v).map_err(|_| ApiError::Unauthorized)?;

    // Manual, constant-time nonce check (replay / token-injection defence).
    let claim_nonce = data.claims.nonce.as_deref().ok_or(ApiError::Unauthorized)?;
    if !crate::ingest_util::constant_time_eq(claim_nonce.as_bytes(), expected_nonce.as_bytes()) {
        return Err(ApiError::Unauthorized);
    }

    Ok(data.claims)
}

// ── handlers ────────────────────────────────────────────────────────────────

async fn config_endpoint() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "enabled": config().is_some() }))
}

async fn login(State(app): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    let cfg = config().ok_or(ApiError::NotFound)?;
    let allow_hosts = issuer_host(&cfg);
    let client = guarded_client(allow_hosts.clone())?;

    let disco_url = format!("{}/.well-known/openid-configuration", cfg.issuer);
    preflight(&disco_url, &allow_hosts).await?;
    let disco = discover(&client, &cfg.issuer).await?;
    preflight(&disco.authorization_endpoint, &allow_hosts).await?;

    let verifier = rand_token();
    let challenge = B64URL.encode(Sha256::digest(verifier.as_bytes()));
    let state = rand_token();
    let nonce = rand_token();
    app.store().stash_oidc_state(&state, &verifier, Some(&nonce), None).await?;

    let auth_url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&nonce={}&code_challenge={}&code_challenge_method=S256",
        disco.authorization_endpoint,
        urlencoding(&cfg.client_id),
        urlencoding(&cfg.redirect_url),
        urlencoding("openid email profile"),
        urlencoding(&state),
        urlencoding(&nonce),
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
    let state = q.state.ok_or(ApiError::BadRequest("missing state".into()))?;
    // Atomic, one-time-use consume of the login state (unknown/expired → 401).
    let consumed = app
        .store()
        .consume_oidc_state(&state)
        .await?
        .ok_or(ApiError::Unauthorized)?;
    let verifier = consumed.pkce_verifier;
    let nonce = consumed.nonce.ok_or(ApiError::Unauthorized)?;

    let allow_hosts = issuer_host(&cfg);
    let client = guarded_client(allow_hosts.clone())?;

    let disco_url = format!("{}/.well-known/openid-configuration", cfg.issuer);
    preflight(&disco_url, &allow_hosts).await?;
    let disco = discover(&client, &cfg.issuer).await?;
    preflight(&disco.token_endpoint, &allow_hosts).await?;
    preflight(&disco.userinfo_endpoint, &allow_hosts).await?;
    preflight(&disco.jwks_uri, &allow_hosts).await?;

    // Exchange the code for tokens (PKCE + confidential client).
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

    let id_token = token.id_token.ok_or(ApiError::Unauthorized)?;

    // Verify the id_token signature + claims + nonce. This is the trust anchor:
    // identity is taken from these verified claims, with userinfo as a fallback.
    let claims =
        validate_id_token(&client, &disco.jwks_uri, &allow_hosts, &id_token, &cfg, &nonce).await?;

    // ── Identity: id_token-primary, userinfo fallback ───────────────────────
    let mut email = claims.email.clone();
    let mut name = claims.name.clone();
    let mut email_verified = claims.email_verified;
    // Org-claim: prefer the id_token's claim set; fall back to userinfo below.
    let mut org_extra = claims.extra.clone();

    // Fetch userinfo only when a needed claim is missing from the id_token.
    let org_claim_present = cfg
        .org_claim
        .as_deref()
        .map(|c| org_extra.contains_key(c))
        .unwrap_or(true);
    let need_userinfo = email.is_none()
        || name.is_none()
        || email_verified.is_none()
        || !org_claim_present;
    if need_userinfo {
        let info: UserInfo = client
            .get(&disco.userinfo_endpoint)
            .bearer_auth(&token.access_token)
            .send()
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("oidc userinfo: {e}")))?
            .json()
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("oidc userinfo parse: {e}")))?;
        email = email.or(info.email);
        name = name.or(info.name);
        email_verified = email_verified.or(info.email_verified);
        // Merge any claims the id_token lacked (id_token values win).
        for (k, val) in info.extra {
            org_extra.entry(k).or_insert(val);
        }
    }

    // Refuse to trust an unverified email — otherwise an attacker who registers
    // an unverified account at the IdP under a victim's address could log in as
    // (or provision) that victim's Rampart user.
    if email_verified != Some(true) {
        return Err(ApiError::BadRequest(
            "oidc: provider did not assert a verified email (email_verified=true); \
             refusing to provision or link an account"
                .into(),
        ));
    }

    // Phase 4f: resolve the configured org-mapping claim (empty unless
    // RAMPART_OIDC_ORG_CLAIM is set), from the merged claim set.
    let org_slugs: Vec<String> = cfg
        .org_claim
        .as_deref()
        .map(|c| claim_org_slugs(&org_extra, c))
        .unwrap_or_default();

    let email = email
        .map(|e| e.to_lowercase())
        .filter(|e| e.contains('@'))
        .ok_or(ApiError::BadRequest("oidc: no email in id_token/userinfo".into()))?;

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
                    name: Some(name.unwrap_or_else(|| email.clone())),
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
    // auto-create, no deny). Idempotent. No-op when org_slugs is empty.
    let mut mapped_org: Option<rampart_core::ids::OrgId> = None;
    for slug in &org_slugs {
        if let Ok(org) = rampart_db::orgs::get_by_slug(app.pool(), slug).await {
            rampart_db::orgs::upsert_member(app.pool(), org.id, user.id, cfg.default_role).await?;
            if mapped_org.is_none() {
                mapped_org = Some(org.id);
            }
        }
    }

    rampart_db::users::mark_login(app.pool(), user.id).await.ok();

    let session = rampart_db::sessions::create(
        app.pool(),
        user.id,
        SESSION_TTL_SECS,
        crate::client_ip::from_headers(&headers),
        headers
            .get("user-agent")
            .and_then(|v| v.to_str().ok())
            .map(String::from),
    )
    .await?;

    // Phase 4f: point the new session at the first mapped org. Best-effort.
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
    fn asym_allow_list_rejects_symmetric_and_none() {
        // HMAC algorithms must not be on the allow-list (alg-confusion defence).
        assert!(!ASYM.contains(&Algorithm::HS256));
        assert!(!ASYM.contains(&Algorithm::HS384));
        assert!(!ASYM.contains(&Algorithm::HS512));
        // The asymmetric families we accept are present.
        for a in [
            Algorithm::RS256,
            Algorithm::PS256,
            Algorithm::ES256,
            Algorithm::EdDSA,
        ] {
            assert!(ASYM.contains(&a));
        }
    }

    #[test]
    fn pick_jwk_uses_sole_key_when_no_kid() {
        let set: JwkSet = serde_json::from_value(serde_json::json!({
            "keys": [{
                "kty": "RSA", "kid": "k1", "n": "abc", "e": "AQAB"
            }]
        }))
        .unwrap();
        // No kid → the single key is selected.
        assert!(pick_jwk(&set, None).is_some());
        // Matching kid → found.
        assert!(pick_jwk(&set, Some("k1")).is_some());
        // Unknown kid → None.
        assert!(pick_jwk(&set, Some("nope")).is_none());
    }

    #[test]
    fn pick_jwk_requires_kid_when_multiple_keys() {
        let set: JwkSet = serde_json::from_value(serde_json::json!({
            "keys": [
                {"kty": "RSA", "kid": "k1", "n": "abc", "e": "AQAB"},
                {"kty": "RSA", "kid": "k2", "n": "def", "e": "AQAB"}
            ]
        }))
        .unwrap();
        // No kid + multiple keys → ambiguous → None.
        assert!(pick_jwk(&set, None).is_none());
        assert!(pick_jwk(&set, Some("k2")).is_some());
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
        assert_eq!(
            claim_org_slugs(&obj(serde_json::json!({"hd": "Acme.com"})), "hd"),
            vec!["acme-com"]
        );
        assert_eq!(
            claim_org_slugs(
                &obj(serde_json::json!({"groups": ["Acme Corp", "beta", 42, "x"]})),
                "groups"
            ),
            vec!["acme-corp", "beta"]
        );
        assert!(claim_org_slugs(&obj(serde_json::json!({"groups": []})), "nope").is_empty());
        assert!(
            claim_org_slugs(&obj(serde_json::json!({"org": {"k": true}})), "org").is_empty()
        );
    }
}
