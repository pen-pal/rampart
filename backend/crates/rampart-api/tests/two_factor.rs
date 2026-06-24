//! Backend enforcement of the `require_2fa` policy (audit re-rank #12).
//!
//! The gate must (a) block state-changing requests by a user who must enroll but
//! hasn't, yet (b) NEVER lock them out — reads and the `/auth/2fa` enrollment
//! endpoints stay reachable so they can complete enrollment.

mod common;

use axum::http::{Method, StatusCode};
use common::{register_admin, request};
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test(migrations = "../../migrations")]
async fn require_2fa_gate_blocks_mutations_but_allows_enrollment(pool: PgPool) {
    let router = common::router(pool.clone());
    // First user = global admin, NOT 2FA-enrolled.
    let admin = register_admin(&router).await;

    // Policy off (default): a mutation works.
    let (s, _, _) = request(
        &router,
        Method::POST,
        "/v1/tags",
        Some(json!({ "name": "t1", "color": "#ffffff" })),
        Some(&admin),
    )
    .await;
    assert!(
        s.is_success(),
        "mutation allowed while require_2fa is off: {s}"
    );

    // Turn enforcement on for everyone.
    rampart_db::settings::put(&pool, "require_2fa", &json!("all"))
        .await
        .unwrap();

    // The unenrolled admin can no longer mutate.
    let (s, _, _) = request(
        &router,
        Method::POST,
        "/v1/tags",
        Some(json!({ "name": "t2", "color": "#ffffff" })),
        Some(&admin),
    )
    .await;
    assert_eq!(
        s,
        StatusCode::FORBIDDEN,
        "unenrolled mutation must be blocked"
    );

    // …but reads still work (the SPA renders the enroll prompt) …
    let (s, _, _) = request(&router, Method::GET, "/v1/tags", None, Some(&admin)).await;
    assert!(s.is_success(), "reads stay allowed: {s}");

    // …and the 2FA enrollment endpoint stays reachable (no lockout).
    let (s, _, _) = request(
        &router,
        Method::POST,
        "/v1/auth/2fa/setup",
        None,
        Some(&admin),
    )
    .await;
    assert_ne!(
        s,
        StatusCode::FORBIDDEN,
        "the user must always be able to enroll"
    );
}
