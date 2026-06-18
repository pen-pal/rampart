//! Per-API-key scope enforcement (migration 0057).
//!
//! Until 0057 every API key authenticated a request with FULL access — a
//! real security gap. A key now carries one `scope` that maps onto the RBAC
//! roles, and the existing route guards enforce it:
//!   - read  → Role::Readonly: GET ok, any mutation 403
//!   - write → Role::Editor:   monitor CRUD ok, admin-only routes 403
//!   - admin → Role::Admin:    everything, incl. api-key management
//!
//! The key's scope is authoritative regardless of the creator's user role:
//! an admin can mint a `read` key and that key still gets 403 on mutations.

mod common;

use axum::body::{Body, Bytes};
use axum::http::{Method, Request, Response, StatusCode};
use axum::Router;
use common::{register_admin, router};
use http_body_util::BodyExt;
use rampart_core::api_key::{KeyScope, NewApiKey};
use rampart_core::Role;
use serde_json::json;
use sqlx::PgPool;
use tower::ServiceExt;

const TEST_ORG: rampart_core::ids::OrgId =
    rampart_core::ids::OrgId::from_uuid(rampart_core::org::DEFAULT_ORG_ID);

fn new_monitor_body() -> serde_json::Value {
    json!({
        "name": "Example",
        "kind": "http",
        "url": "https://example.com",
        "interval_seconds": 60
    })
}

/// Mint an API key with the given scope, owned by a freshly-created user,
/// and return the raw bearer token. The owning user is created as an admin
/// to prove the KEY scope — not the creator's role — is what's enforced.
async fn key_with_scope(pool: &PgPool, email: &str, scope: KeyScope) -> String {
    let owner = rampart_db::users::create(
        pool,
        rampart_db::users::NewUser {
            email: email.into(),
            name: Some("Owner".into()),
            password_hash: "$argon2id$v=19$m=19456,t=2,p=1$fake$hash".into(),
            role: Role::Admin,
        },
    )
    .await
    .expect("create key owner");

    let issued = rampart_db::api_keys::create(
        pool,
        NewApiKey {
            name: format!("{}-key", scope.as_str()),
            scope,
            expires_at: None,
            rate_limit_per_hour: rampart_core::api_key::DEFAULT_RATE_LIMIT_PER_HOUR,
        },
        owner.id,
        TEST_ORG,
    )
    .await
    .expect("mint api key");
    issued.token
}

/// Issue a request authenticated with an `Authorization: Bearer <token>`
/// API key (the script/automation flow), mirroring `common::request`'s
/// cookie path but for bearer auth.
async fn bearer_request(
    router: &Router,
    method: Method,
    path: &str,
    body_json: Option<serde_json::Value>,
    token: &str,
) -> (StatusCode, Bytes) {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {token}"));
    if body_json.is_some() {
        req = req.header("content-type", "application/json");
    }
    let body = match body_json {
        Some(v) => Body::from(serde_json::to_vec(&v).unwrap()),
        None => Body::empty(),
    };
    let resp: Response<Body> = router
        .clone()
        .oneshot(req.body(body).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, bytes)
}

#[sqlx::test(migrations = "../../migrations")]
async fn read_key_can_get_but_not_mutate(pool: PgPool) {
    let app = router(pool.clone());
    register_admin(&app).await;
    let token = key_with_scope(&pool, "read@example.com", KeyScope::Read).await;

    // GET is allowed.
    let (status, _) = bearer_request(&app, Method::GET, "/v1/monitors", None, &token).await;
    assert_eq!(status, StatusCode::OK, "read key GET /v1/monitors");

    // POST (create) is forbidden — this is the gap 0057 closes.
    let (status, body) = bearer_request(
        &app,
        Method::POST,
        "/v1/monitors",
        Some(new_monitor_body()),
        &token,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "read key POST should 403, got body: {}",
        String::from_utf8_lossy(&body)
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn write_key_can_mutate_but_not_admin_routes(pool: PgPool) {
    let app = router(pool.clone());
    register_admin(&app).await;
    let token = key_with_scope(&pool, "write@example.com", KeyScope::Write).await;

    // Editor-equivalent: monitor CRUD works.
    let (status, body) = bearer_request(
        &app,
        Method::POST,
        "/v1/monitors",
        Some(new_monitor_body()),
        &token,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "write key POST /v1/monitors should 201, got body: {}",
        String::from_utf8_lossy(&body)
    );

    // But the admin-only routes are off-limits (api-keys is admin-gated).
    let (status, _) = bearer_request(&app, Method::GET, "/v1/api-keys", None, &token).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "write key GET /v1/api-keys (admin route) should 403"
    );
    let (status, _) = bearer_request(&app, Method::GET, "/v1/users", None, &token).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "write key GET /v1/users (admin route) should 403"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn admin_key_has_full_access(pool: PgPool) {
    let app = router(pool.clone());
    register_admin(&app).await;
    // NB: a distinct email — register_admin already claimed
    // admin@example.com as the first-run admin, so the key owner needs
    // its own address or `create` returns Conflict.
    let token = key_with_scope(&pool, "adminkey@example.com", KeyScope::Admin).await;

    // Reads.
    let (status, _) = bearer_request(&app, Method::GET, "/v1/monitors", None, &token).await;
    assert_eq!(status, StatusCode::OK, "admin key GET /v1/monitors");

    // Mutations.
    let (status, body) = bearer_request(
        &app,
        Method::POST,
        "/v1/monitors",
        Some(new_monitor_body()),
        &token,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "admin key POST /v1/monitors should 201, got body: {}",
        String::from_utf8_lossy(&body)
    );

    // Admin-only routes.
    let (status, _) = bearer_request(&app, Method::GET, "/v1/api-keys", None, &token).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "admin key GET /v1/api-keys (admin route)"
    );
    let (status, _) = bearer_request(&app, Method::GET, "/v1/users", None, &token).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "admin key GET /v1/users (admin route)"
    );
}
