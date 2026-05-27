//! End-to-end-ish tests for `/v1/auth/*` via the in-process axum router.

mod common;

use axum::http::{Method, StatusCode};
use common::{register_admin, request, session_cookie};
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test(migrations = "../../migrations")]
async fn me_reports_needs_setup_on_fresh_db(pool: PgPool) {
    let r = common::router(pool);
    let (status, _, bytes) = request(&r, Method::GET, "/v1/auth/me", None, None).await;
    assert_eq!(status, StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["needs_setup"], json!(true));
}

#[sqlx::test(migrations = "../../migrations")]
async fn protected_route_unauthorized_without_cookie(pool: PgPool) {
    let r = common::router(pool);
    let (status, _, _) = request(&r, Method::GET, "/v1/monitors", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "../../migrations")]
async fn register_then_protected_route_with_cookie(pool: PgPool) {
    let r = common::router(pool);
    let cookie = register_admin(&r).await;
    let (status, _, _) = request(&r, Method::GET, "/v1/monitors", None, Some(&cookie)).await;
    assert_eq!(status, StatusCode::OK);
}

#[sqlx::test(migrations = "../../migrations")]
async fn second_register_returns_conflict(pool: PgPool) {
    let r = common::router(pool);
    register_admin(&r).await;
    let (status, _, _) = request(
        &r,
        Method::POST,
        "/v1/auth/register",
        Some(json!({ "email": "second@x.com", "password": "ten-character-pw" })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[sqlx::test(migrations = "../../migrations")]
async fn register_rejects_short_password(pool: PgPool) {
    let r = common::router(pool);
    let (status, _, _) = request(
        &r,
        Method::POST,
        "/v1/auth/register",
        Some(json!({ "email": "x@x.com", "password": "short" })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "../../migrations")]
async fn register_rejects_invalid_email(pool: PgPool) {
    let r = common::router(pool);
    let (status, _, _) = request(
        &r,
        Method::POST,
        "/v1/auth/register",
        Some(json!({ "email": "no-at-sign", "password": "ten-character-pw" })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "../../migrations")]
async fn login_wrong_password_returns_unauthorized(pool: PgPool) {
    let r = common::router(pool);
    register_admin(&r).await;
    let (status, _, _) = request(
        &r,
        Method::POST,
        "/v1/auth/login",
        Some(json!({ "email": "admin@example.com", "password": "totally-wrong-password" })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "../../migrations")]
async fn login_right_password_sets_cookie(pool: PgPool) {
    let r = common::router(pool);
    register_admin(&r).await;
    let (status, headers, _) = request(
        &r,
        Method::POST,
        "/v1/auth/login",
        Some(json!({ "email": "admin@example.com", "password": "correct-horse-battery-staple" })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let cookie = session_cookie(&headers);
    assert!(cookie.starts_with("rampart_session="), "got: {cookie:?}");
}

#[sqlx::test(migrations = "../../migrations")]
async fn logout_clears_cookie_and_subsequent_request_401s(pool: PgPool) {
    let r = common::router(pool);
    let cookie = register_admin(&r).await;

    // Confirm session works pre-logout.
    let (s1, _, _) = request(&r, Method::GET, "/v1/monitors", None, Some(&cookie)).await;
    assert_eq!(s1, StatusCode::OK);

    // Logout. The server returns a Set-Cookie clearing the session, and
    // also deletes the session row in the DB.
    let (s2, _, _) = request(&r, Method::POST, "/v1/auth/logout", None, Some(&cookie)).await;
    assert_eq!(s2, StatusCode::NO_CONTENT);

    // Re-using the old cookie should now 401 because the session row is gone.
    let (s3, _, _) = request(&r, Method::GET, "/v1/monitors", None, Some(&cookie)).await;
    assert_eq!(s3, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "../../migrations")]
async fn health_endpoints_are_public(pool: PgPool) {
    let r = common::router(pool);
    for path in ["/healthz", "/readyz"] {
        let (status, _, _) = request(&r, Method::GET, path, None, None).await;
        assert_eq!(status, StatusCode::OK, "{path} should be public");
    }
}
