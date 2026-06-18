//! Phase 5: telemetry ingest stamps the org resolved from the ingest
//! credential. An OTLP request bearing an org-scoped ingest key lands in that
//! org; a token-less request (no key, no global token) falls back to Default.

mod common;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use common::router;
use rampart_core::ids::OrgId;
use sqlx::PgPool;
use tower::ServiceExt;

const OTHER: &str = "00000000-0000-0000-0000-0000000c0ffe";
const DEFAULT_ORG: &str = "00000000-0000-0000-0000-000000000001";

// Minimal OTLP/JSON logs payload → exactly one ParsedLog (service "demo").
const OTLP_LOG: &str = r#"{"resourceLogs":[{"resource":{"attributes":[{"key":"service.name","value":{"stringValue":"demo"}}]},"scopeLogs":[{"logRecords":[{"timeUnixNano":"1700000000000000000","severityNumber":9,"severityText":"INFO","body":{"stringValue":"hello"}}]}]}]}"#;

async fn post_logs(app: &axum::Router, token: Option<&str>) -> StatusCode {
    let mut b = Request::builder()
        .method(Method::POST)
        .uri("/otlp/v1/logs")
        .header("content-type", "application/json");
    if let Some(t) = token {
        b = b.header("x-rampart-token", t);
    }
    app.clone()
        .oneshot(b.body(Body::from(OTLP_LOG)).unwrap())
        .await
        .unwrap()
        .status()
}

async fn count_logs(pool: &PgPool, org: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM logs WHERE org_id = $1::uuid")
        .bind(org)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn otlp_logs_stamp_org_from_ingest_key(pool: PgPool) {
    let app = router(pool.clone());

    // A second org + an ingest key scoped to it.
    sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1::uuid, 'other', 'Other') ON CONFLICT DO NOTHING")
        .bind(OTHER)
        .execute(&pool)
        .await
        .unwrap();
    let other = OrgId::from_uuid(uuid::Uuid::parse_str(OTHER).unwrap());
    let (_key, token) = rampart_db::ingest_keys::create(&pool, other, "collector", "all", &[])
        .await
        .unwrap();

    // OTLP logs bearing the org key → the log lands in that org.
    assert_eq!(post_logs(&app, Some(&token)).await, StatusCode::OK);
    assert_eq!(count_logs(&pool, OTHER).await, 1, "ingest-key org stamped");
    assert_eq!(count_logs(&pool, DEFAULT_ORG).await, 0, "nothing misfiled to Default");

    // Token-less (no key, no global telemetry_token configured) → open ingest,
    // falls back to the Default org (single-org behaviour preserved).
    assert_eq!(post_logs(&app, None).await, StatusCode::OK);
    assert_eq!(count_logs(&pool, DEFAULT_ORG).await, 1, "tokenless → Default");
    assert_eq!(count_logs(&pool, OTHER).await, 1, "Default ingest didn't touch the other org");
}

async fn post_rum(app: &axum::Router, token: &str, origin: &str) -> StatusCode {
    app.clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/rum/v1/events?k={token}"))
                .header("content-type", "application/json")
                .header("origin", origin)
                .body(Body::from(
                    r#"{"url":"https://app.example/","app":"web","metrics":{"lcp":1200}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

#[sqlx::test(migrations = "../../migrations")]
async fn rum_origin_binding_and_org_stamp(pool: PgPool) {
    let app = router(pool.clone());
    sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1::uuid, 'other', 'Other') ON CONFLICT DO NOTHING")
        .bind(OTHER)
        .execute(&pool)
        .await
        .unwrap();
    let other = OrgId::from_uuid(uuid::Uuid::parse_str(OTHER).unwrap());
    // An origin-bound RUM key (public ?k token).
    let (_k, token) = rampart_db::ingest_keys::create(
        &pool,
        other,
        "rum",
        "rum",
        &["https://good.example".to_string()],
    )
    .await
    .unwrap();

    // Forged/leaked key used from a disallowed origin → rejected.
    assert_eq!(
        post_rum(&app, &token, "https://bad.example").await,
        StatusCode::UNAUTHORIZED,
        "wrong Origin → 401 (origin-binding)"
    );
    // Allowed origin → accepted, beacon stamped with the key's org.
    assert_eq!(
        post_rum(&app, &token, "https://good.example").await,
        StatusCode::NO_CONTENT
    );
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM rum_events WHERE org_id = $1::uuid")
        .bind(OTHER)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1, "RUM beacon stamped with the ingest key's org");
}
