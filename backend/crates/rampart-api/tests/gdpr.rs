//! GDPR data export + right-to-erasure integration tests.
//!
//! Verifies the compliance slice end-to-end against a real router:
//!   - export aggregates the user's personal data,
//!   - erase anonymizes the PII in place (row kept — audit chain intact),
//!     revokes access (login fails afterwards),
//!   - you cannot erase your own account.

mod common;

use axum::http::{Method, StatusCode};
use common::{json, register_admin, request, router};
use serde_json::{json as j, Value};
use sqlx::PgPool;

#[sqlx::test(migrations = "../../migrations")]
async fn export_then_erase_anonymizes_and_revokes(pool: PgPool) {
    let app = router(pool.clone());
    let admin = register_admin(&app).await;

    // Create a target user through the admin API (defaults to editor).
    let (st, _, body) = request(
        &app,
        Method::POST,
        "/v1/users",
        Some(j!({ "email": "bob@example.com", "name": "Bob", "password": "correct-horse-battery-staple" })),
        Some(&admin),
    )
    .await;
    assert_eq!(
        st,
        StatusCode::CREATED,
        "create user: {}",
        String::from_utf8_lossy(&body)
    );
    let bob: Value = serde_json::from_slice(&body).unwrap();
    let bob_id = bob["id"].as_str().unwrap().to_string();

    // Bob can authenticate pre-erasure.
    let (st, _, _) = request(
        &app,
        Method::POST,
        "/v1/auth/login",
        Some(j!({ "email": "bob@example.com", "password": "correct-horse-battery-staple" })),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "bob login pre-erase");

    // Export returns Bob's personal data.
    let (st, _, body) = request(
        &app,
        Method::GET,
        &format!("/v1/users/{bob_id}/export"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "export");
    let exp: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        exp["user"]["email"], "bob@example.com",
        "export carries email"
    );
    assert!(
        exp.get("sessions").is_some() && exp.get("organizations").is_some(),
        "export shape"
    );

    // Erase (anonymize).
    let (st, _, body) = request(
        &app,
        Method::POST,
        &format!("/v1/users/{bob_id}/erase"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(
        st,
        StatusCode::NO_CONTENT,
        "erase: {}",
        String::from_utf8_lossy(&body)
    );

    // Row survives but is tombstoned (PII scrubbed) — audit chain stays intact.
    let users: Value = json(&app, Method::GET, "/v1/users", None, Some(&admin)).await;
    let erased = users
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["id"] == bob_id)
        .expect("erased user row still present (anonymized, not deleted)");
    assert!(
        erased["email"].as_str().unwrap().starts_with("erased-"),
        "email tombstoned, got {}",
        erased["email"]
    );
    assert!(erased["name"].is_null(), "display name cleared");

    // Bob can no longer authenticate (tombstoned email + dead password).
    let (st, _, _) = request(
        &app,
        Method::POST,
        "/v1/auth/login",
        Some(j!({ "email": "bob@example.com", "password": "correct-horse-battery-staple" })),
        None,
    )
    .await;
    assert_eq!(
        st,
        StatusCode::UNAUTHORIZED,
        "bob login post-erase must fail"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn cannot_erase_self(pool: PgPool) {
    let app = router(pool.clone());
    let admin = register_admin(&app).await;
    let me: Value = json(&app, Method::GET, "/v1/auth/me", None, Some(&admin)).await;
    let my_id = me["user"]["id"].as_str().unwrap().to_string();
    let (st, _, _) = request(
        &app,
        Method::POST,
        &format!("/v1/users/{my_id}/erase"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "self-erase must 400");
}
