//! Compliance access-review report: admin-gated, returns the cross-org
//! membership snapshot as JSON + CSV, and 403s for non-admins.

mod common;

use axum::http::{Method, StatusCode};
use common::{register_admin, request, router, user_with_role};
use rampart_core::Role;
use serde_json::Value;
use sqlx::PgPool;

#[sqlx::test(migrations = "../../migrations")]
async fn access_review_admin_only_json_and_csv(pool: PgPool) {
    let app = router(pool.clone());
    let admin = register_admin(&app).await;

    // Admin sees the report; the admin's own Default-org membership is present.
    let (st, _, body) = request(
        &app,
        Method::GET,
        "/v1/compliance/access-review",
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "admin access-review");
    let rows: Value = serde_json::from_slice(&body).unwrap();
    let arr = rows.as_array().expect("array");
    assert!(!arr.is_empty(), "at least the admin's membership");
    assert!(
        arr.iter()
            .any(|r| r["role"].as_str() == Some("admin") && r["email"].is_string()),
        "admin grant present with identity"
    );

    // CSV variant: admin gets a text/csv attachment.
    let (st, headers, body) = request(
        &app,
        Method::GET,
        "/v1/compliance/access-review.csv",
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "admin access-review.csv");
    let ct = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.as_str())
        .unwrap_or("");
    assert!(ct.contains("text/csv"), "csv content-type, got {ct}");
    let text = String::from_utf8_lossy(&body);
    assert!(text.starts_with("org_slug,org_name,email"), "csv header");

    // A non-admin (editor) is refused.
    let editor = user_with_role(&pool, "editor@example.com", Role::Editor).await;
    let (st, _, _) = request(
        &app,
        Method::GET,
        "/v1/compliance/access-review",
        None,
        Some(&editor),
    )
    .await;
    assert_eq!(
        st,
        StatusCode::FORBIDDEN,
        "editor blocked from access-review"
    );
}

// The report-pull is itself audited (compliance.access_review).
#[sqlx::test(migrations = "../../migrations")]
async fn access_review_is_audited(pool: PgPool) {
    let app = router(pool.clone());
    let admin = register_admin(&app).await;
    let (st, _, _) = request(
        &app,
        Method::GET,
        "/v1/compliance/access-review",
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_log WHERE action = 'compliance.access_review'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 1, "access-review pull recorded in the audit log");
}
