//! Per-API-key rate-limit headers (item 11).
//!
//! An API-key-authenticated request gets the advisory `X-RateLimit-*`
//! header set (Limit / Remaining / Reset). A cookie/session request stays
//! unlimited and gets NONE of those headers — the middleware keys off the
//! `AuthApiKeyId` extension that only the bearer auth path stamps.

mod common;

use axum::body::{Body, Bytes};
use axum::http::{Method, Request, Response, StatusCode};
use axum::Router;
use common::{register_admin, router};
use http_body_util::BodyExt;
use rampart_core::api_key::{KeyScope, NewApiKey};
use rampart_core::Role;
use sqlx::PgPool;
use tower::ServiceExt;

const TEST_ORG: rampart_core::ids::OrgId =
    rampart_core::ids::OrgId::from_uuid(rampart_core::org::DEFAULT_ORG_ID);

/// Mint an admin-scope API key owned by a fresh admin user, with the given
/// per-hour budget; return the raw bearer token.
async fn admin_key_with_budget(pool: &PgPool, email: &str, rate_limit_per_hour: i32) -> String {
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
            name: "rl-key".into(),
            scope: KeyScope::Admin,
            expires_at: None,
            rate_limit_per_hour,
        },
        owner.id,
        TEST_ORG,
    )
    .await
    .expect("mint api key");
    issued.token
}

/// Mint an admin-scope API key with the default (1000/hr) budget.
async fn admin_key(pool: &PgPool, email: &str) -> String {
    admin_key_with_budget(
        pool,
        email,
        rampart_core::api_key::DEFAULT_RATE_LIMIT_PER_HOUR,
    )
    .await
}

/// GET `path` with a bearer token, returning status + all response headers.
async fn bearer_get(
    router: &Router,
    path: &str,
    token: &str,
) -> (StatusCode, Vec<(String, String)>, Bytes) {
    let req = Request::builder()
        .method(Method::GET)
        .uri(path)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp: Response<Body> = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let headers = resp
        .headers()
        .iter()
        .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, headers, bytes)
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

#[sqlx::test(migrations = "../../migrations")]
async fn api_key_request_gets_ratelimit_headers(pool: PgPool) {
    let app = router(pool.clone());
    // Register the first admin so the system isn't in the locked
    // bootstrap state, then mint a key.
    let _ = register_admin(&app).await;
    let token = admin_key(&pool, "keyowner@example.com").await;

    // A protected GET authenticated by the api key.
    let (status, headers, body) = bearer_get(&app, "/v1/monitors", &token).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "api-key GET should succeed: {}",
        String::from_utf8_lossy(&body)
    );

    // All three advisory headers present.
    assert_eq!(
        header(&headers, "x-ratelimit-limit"),
        Some("1000"),
        "limit header"
    );
    let remaining: u32 = header(&headers, "x-ratelimit-remaining")
        .expect("remaining header present")
        .parse()
        .expect("remaining is a number");
    // First request in the window → 999 remaining out of 1000.
    assert_eq!(remaining, 999, "remaining after one request");
    assert!(
        header(&headers, "x-ratelimit-reset").is_some(),
        "reset header present"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn ratelimit_limit_header_reflects_per_key_budget(pool: PgPool) {
    let app = router(pool.clone());
    let _ = register_admin(&app).await;
    // Mint a key with a small, non-default budget.
    let token = admin_key_with_budget(&pool, "smallbudget@example.com", 5).await;

    let (status, headers, body) = bearer_get(&app, "/v1/monitors", &token).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "api-key GET should succeed: {}",
        String::from_utf8_lossy(&body)
    );

    // The advertised ceiling matches the key's own budget, not the default.
    assert_eq!(
        header(&headers, "x-ratelimit-limit"),
        Some("5"),
        "limit header reflects per-key budget"
    );
    let remaining: u32 = header(&headers, "x-ratelimit-remaining")
        .expect("remaining header present")
        .parse()
        .expect("remaining is a number");
    // First request in the window → 4 remaining out of 5.
    assert_eq!(
        remaining, 4,
        "remaining after one request against a budget of 5"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn ratelimit_remaining_decrements_across_requests(pool: PgPool) {
    let app = router(pool.clone());
    let _ = register_admin(&app).await;
    let token = admin_key_with_budget(&pool, "counter@example.com", 10).await;

    // Three successive requests on the same key. The counter is now durable
    // (DB-backed), so each request decrements `remaining` monotonically
    // within the fixed window — 9, 8, 7 out of a budget of 10.
    for expected_remaining in [9_u32, 8, 7] {
        let (status, headers, body) = bearer_get(&app, "/v1/monitors", &token).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "api-key GET should succeed: {}",
            String::from_utf8_lossy(&body)
        );
        let remaining: u32 = header(&headers, "x-ratelimit-remaining")
            .expect("remaining header present")
            .parse()
            .expect("remaining is a number");
        assert_eq!(
            remaining, expected_remaining,
            "remaining must decrement across requests on the same key"
        );
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn over_budget_request_is_throttled(pool: PgPool) {
    let app = router(pool.clone());
    let _ = register_admin(&app).await;
    // Budget of 1: the first request is admitted, the second exceeds it.
    let token = admin_key_with_budget(&pool, "throttle@example.com", 1).await;

    let (status, _, _) = bearer_get(&app, "/v1/monitors", &token).await;
    assert_eq!(status, StatusCode::OK, "first request under budget");

    let (status, headers, _) = bearer_get(&app, "/v1/monitors", &token).await;
    assert_eq!(
        status,
        StatusCode::TOO_MANY_REQUESTS,
        "second request over budget is 429"
    );
    assert_eq!(
        header(&headers, "x-ratelimit-remaining"),
        Some("0"),
        "remaining is 0 when throttled"
    );
    assert!(
        header(&headers, "retry-after").is_some(),
        "Retry-After present on 429"
    );
    assert_eq!(
        header(&headers, "x-ratelimit-limit"),
        Some("1"),
        "limit header still advertised on 429"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn session_request_has_no_ratelimit_headers(pool: PgPool) {
    let app = router(pool.clone());
    let cookie = register_admin(&app).await;

    // Same protected GET, but cookie-authenticated.
    let (status, headers, _) =
        common::request(&app, Method::GET, "/v1/monitors", None, Some(&cookie)).await;
    assert_eq!(status, StatusCode::OK, "session GET should succeed");

    assert!(
        header(&headers, "x-ratelimit-limit").is_none(),
        "session request must NOT carry x-ratelimit-limit"
    );
    assert!(
        header(&headers, "x-ratelimit-remaining").is_none(),
        "session request must NOT carry x-ratelimit-remaining"
    );
    assert!(
        header(&headers, "x-ratelimit-reset").is_none(),
        "session request must NOT carry x-ratelimit-reset"
    );
}
