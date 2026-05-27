//! Shared helpers for API integration tests.
//!
//! Each test uses `sqlx::test` to get its own isolated database; we
//! turn that pool into a `Router` and provide a small `request()` helper
//! that drives the router via `tower::ServiceExt::oneshot` (no TCP
//! listener — fast + parallel-safe).

#![allow(dead_code)]

use axum::body::{Body, Bytes};
use axum::http::{Method, Request, Response, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use rampart_api::test_router;
use serde::de::DeserializeOwned;
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;

pub fn router(pool: PgPool) -> Router {
    test_router(pool)
}

/// Issue a JSON request against the router. `body_json` is serialised;
/// `cookie` is sent verbatim as the Cookie header value if Some.
pub async fn request(
    router: &Router,
    method: Method,
    path:   &str,
    body_json: Option<Value>,
    cookie: Option<&str>,
) -> (StatusCode, Vec<(String, String)>, Bytes) {
    let mut req = Request::builder().method(method).uri(path);
    if let Some(c) = cookie { req = req.header("cookie", c); }
    if let Some(b) = &body_json {
        req = req.header("content-type", "application/json");
    }
    let body = match body_json {
        Some(v) => Body::from(serde_json::to_vec(&v).unwrap()),
        None    => Body::empty(),
    };
    let resp: Response<Body> = router.clone().oneshot(req.body(body).unwrap()).await.unwrap();
    let status  = resp.status();
    let headers = resp.headers().iter()
        .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();
    let bytes   = resp.into_body().collect().await.unwrap().to_bytes();
    (status, headers, bytes)
}

/// Helper that asserts 2xx and deserialises the body into `T`. Panics on
/// mismatch with a clear message — these are tests, not production.
pub async fn json<T: DeserializeOwned>(
    router: &Router, method: Method, path: &str,
    body: Option<Value>, cookie: Option<&str>,
) -> T {
    let (status, _, bytes) = request(router, method, path, body, cookie).await;
    assert!(status.is_success(), "expected 2xx, got {status}: {}", String::from_utf8_lossy(&bytes));
    serde_json::from_slice(&bytes).unwrap_or_else(|e| {
        panic!("could not deserialize body: {e}\nbody = {}", String::from_utf8_lossy(&bytes))
    })
}

/// Extract the Set-Cookie session token value so subsequent requests can
/// be authenticated.
pub fn session_cookie(headers: &[(String, String)]) -> String {
    let raw = headers.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("set-cookie"))
        .map(|(_, v)| v.as_str())
        .unwrap_or("");
    // Strip everything past the first ';' — we just need name=value for the next req.
    raw.split(';').next().unwrap_or("").to_string()
}

/// Register the first admin, return the session cookie value.
pub async fn register_admin(router: &Router) -> String {
    let (status, headers, _) = request(
        router, Method::POST, "/v1/auth/register",
        Some(serde_json::json!({
            "email":    "admin@example.com",
            "name":     "Admin",
            "password": "correct-horse-battery-staple",
        })),
        None,
    ).await;
    assert_eq!(status, StatusCode::CREATED, "register first user");
    session_cookie(&headers)
}

/// Login as the previously-registered admin and return its session cookie.
pub async fn login_admin(router: &Router) -> String {
    let (status, headers, _) = request(
        router, Method::POST, "/v1/auth/login",
        Some(serde_json::json!({
            "email":    "admin@example.com",
            "password": "correct-horse-battery-staple",
        })),
        None,
    ).await;
    assert_eq!(status, StatusCode::OK, "login should succeed");
    session_cookie(&headers)
}
