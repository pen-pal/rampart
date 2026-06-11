//! External metric ingestion (migration 0072): text-format push, series
//! listing, bucketed range reads, RBAC.

mod common;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use common::{register_admin, request, user_with_role};
use http_body_util::BodyExt;
use rampart_core::Role;
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;

/// Raw-body POST (the ingest endpoint takes text, not JSON).
async fn post_text(
    router: &axum::Router,
    path: &str,
    body: &str,
    cookie: &str,
) -> (StatusCode, Vec<u8>) {
    let req = Request::builder()
        .method(Method::POST)
        .uri(path)
        .header("cookie", cookie)
        .header("content-type", "text/plain")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    (status, bytes)
}

#[sqlx::test(migrations = "../../migrations")]
async fn ingest_series_and_query(pool: PgPool) {
    let router = common::router(pool);
    let admin = register_admin(&router).await;

    let payload = "\
# TYPE backup_duration_seconds gauge
backup_duration_seconds 312.5
queue_depth{queue=\"emails\"} 42
queue_depth{queue=\"emails\"} 40
not a metric line
";
    let (status, body) = post_text(&router, "/v1/metrics/ingest", payload, &admin).await;
    assert_eq!(status, StatusCode::OK);
    let out: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(out["accepted"], 3);
    assert_eq!(out["skipped"], 1);

    // Series listing: two distinct series, freshest first.
    let (status, _, body) = request(
        &router,
        Method::GET,
        "/v1/metrics/series",
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let series: Vec<Value> = serde_json::from_slice(&body).unwrap();
    assert_eq!(series.len(), 2);
    let qd = series
        .iter()
        .find(|s| s["name"] == "queue_depth")
        .expect("queue_depth series");
    assert_eq!(qd["labels"]["queue"], "emails");
    assert_eq!(qd["samples"], 2);

    // Range query: one bucket, avg of the two samples.
    let (status, _, body) = request(
        &router,
        Method::GET,
        "/v1/metrics/query?name=queue_depth&labels=%7B%22queue%22%3A%22emails%22%7D&step_seconds=300",
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let points: Vec<Value> = serde_json::from_slice(&body).unwrap();
    assert_eq!(points.len(), 1);
    assert_eq!(points[0]["avg"], 41.0);
    assert_eq!(points[0]["min"], 40.0);
    assert_eq!(points[0]["max"], 42.0);

    // Pure garbage → 400, nothing stored.
    let (status, _) = post_text(&router, "/v1/metrics/ingest", "<<not metrics>>", &admin).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "../../migrations")]
async fn readonly_cannot_push(pool: PgPool) {
    let router = common::router(pool.clone());
    let _admin = register_admin(&router).await;
    let readonly = user_with_role(&pool, "ro@example.com", Role::Readonly).await;

    let (status, _) = post_text(&router, "/v1/metrics/ingest", "m 1", &readonly).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // …but may read.
    let (status, _, _) = request(
        &router,
        Method::GET,
        "/v1/metrics/series",
        None,
        Some(&readonly),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}
