//! `/v1/monitors` integration tests.

mod common;

use axum::http::{Method, StatusCode};
use common::{json, register_admin, request};
use serde_json::{json, Value};
use sqlx::PgPool;

fn new_http(name: &str, url: &str) -> Value {
    json!({
        "name": name, "kind": "http", "url": url,
        "interval_seconds": 60, "timeout_seconds": 10,
        "accepted_statuses": [200],
    })
}

#[sqlx::test(migrations = "../../migrations")]
async fn list_is_empty_after_signup(pool: PgPool) {
    let r = common::router(pool);
    let c = register_admin(&r).await;
    let ms: Vec<Value> = json(&r, Method::GET, "/v1/monitors", None, Some(&c)).await;
    assert!(ms.is_empty());
}

#[sqlx::test(migrations = "../../migrations")]
async fn create_then_list_includes_it(pool: PgPool) {
    let r = common::router(pool);
    let c = register_admin(&r).await;
    let created: Value = json(
        &r,
        Method::POST,
        "/v1/monitors",
        Some(new_http("api", "https://api.example.com")),
        Some(&c),
    )
    .await;
    assert_eq!(created["name"], "api");
    assert_eq!(created["kind"], "http");
    let ms: Vec<Value> = json(&r, Method::GET, "/v1/monitors", None, Some(&c)).await;
    assert_eq!(ms.len(), 1);
}

#[sqlx::test(migrations = "../../migrations")]
async fn pause_resume_delete(pool: PgPool) {
    let r = common::router(pool);
    let c = register_admin(&r).await;
    let created: Value = json(
        &r,
        Method::POST,
        "/v1/monitors",
        Some(new_http("x", "https://x.example.com")),
        Some(&c),
    )
    .await;
    let id = created["id"].as_str().unwrap();

    let (s, _, _) = request(
        &r,
        Method::POST,
        &format!("/v1/monitors/{id}/pause"),
        None,
        Some(&c),
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let m: Value = json(
        &r,
        Method::GET,
        &format!("/v1/monitors/{id}"),
        None,
        Some(&c),
    )
    .await;
    assert_eq!(m["active"], false);

    let (s, _, _) = request(
        &r,
        Method::POST,
        &format!("/v1/monitors/{id}/resume"),
        None,
        Some(&c),
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let m: Value = json(
        &r,
        Method::GET,
        &format!("/v1/monitors/{id}"),
        None,
        Some(&c),
    )
    .await;
    assert_eq!(m["active"], true);

    let (s, _, _) = request(
        &r,
        Method::DELETE,
        &format!("/v1/monitors/{id}"),
        None,
        Some(&c),
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
}

#[sqlx::test(migrations = "../../migrations")]
async fn invalid_id_returns_400(pool: PgPool) {
    let r = common::router(pool);
    let c = register_admin(&r).await;
    let (s, _, _) = request(&r, Method::GET, "/v1/monitors/not-a-uuid", None, Some(&c)).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "../../migrations")]
async fn unknown_id_returns_404(pool: PgPool) {
    let r = common::router(pool);
    let c = register_admin(&r).await;
    let fake = uuid::Uuid::new_v4();
    let (s, _, _) = request(
        &r,
        Method::GET,
        &format!("/v1/monitors/{fake}"),
        None,
        Some(&c),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../../migrations")]
async fn summary_and_history_endpoints_respond(pool: PgPool) {
    let r = common::router(pool);
    let c = register_admin(&r).await;
    // Empty arrays are valid responses pre-data.
    let (s1, _, b1) = request(&r, Method::GET, "/v1/monitors/summary", None, Some(&c)).await;
    assert_eq!(s1, StatusCode::OK);
    assert_eq!(b1.as_ref(), b"[]");

    let (s2, _, b2) = request(
        &r,
        Method::GET,
        "/v1/monitors/history?per=10",
        None,
        Some(&c),
    )
    .await;
    assert_eq!(s2, StatusCode::OK);
    assert_eq!(b2.as_ref(), b"[]");
}

#[sqlx::test(migrations = "../../migrations")]
async fn validation_rejects_below_min_interval(pool: PgPool) {
    let r = common::router(pool);
    let c = register_admin(&r).await;
    let payload = json!({
        "name": "bad", "kind": "http", "url": "https://x.example.com",
        "interval_seconds": 1, "timeout_seconds": 10,
    });
    let (s, _, _) = request(&r, Method::POST, "/v1/monitors", Some(payload), Some(&c)).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test(migrations = "../../migrations")]
async fn bulk_edit_sets_interval_on_all(pool: PgPool) {
    let r = common::router(pool);
    let c = register_admin(&r).await;

    // Create three monitors at the default 60s interval.
    let mut ids = Vec::new();
    for n in 0..3 {
        let created: Value = json(
            &r,
            Method::POST,
            "/v1/monitors",
            Some(new_http(
                &format!("m{n}"),
                &format!("https://m{n}.example.com"),
            )),
            Some(&c),
        )
        .await;
        ids.push(created["id"].as_str().unwrap().to_string());
    }

    // Bulk-set the interval (plus an unknown id to exercise `skipped`).
    let fake = "00000000-0000-0000-0000-000000000000";
    let mut req_ids = ids.clone();
    req_ids.push(fake.to_string());
    let res: Value = json(
        &r,
        Method::POST,
        "/v1/monitors/bulk-edit",
        Some(json!({ "ids": req_ids, "patch": { "interval_secs": 120 } })),
        Some(&c),
    )
    .await;
    assert_eq!(res["updated"], 3);
    assert_eq!(res["skipped"], 1);

    // Every real monitor now reports the new interval.
    for id in &ids {
        let m: Value = json(
            &r,
            Method::GET,
            &format!("/v1/monitors/{id}"),
            None,
            Some(&c),
        )
        .await;
        assert_eq!(m["interval_seconds"], 120, "monitor {id} not updated");
    }
}
