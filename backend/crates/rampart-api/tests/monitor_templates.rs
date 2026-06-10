//! `/v1/monitor-templates` integration tests.

mod common;

use axum::http::{Method, StatusCode};
use common::{json, register_admin, request};
use serde_json::{json, Value};
use sqlx::PgPool;

fn template_payload(name: &str, monitor_name: &str, url: &str) -> Value {
    json!({
        "name": name,
        "description": "reusable http probe",
        "spec": {
            "name": monitor_name,
            "kind": "http",
            "url": url,
            "interval_seconds": 60,
            "timeout_seconds": 10,
        },
    })
}

#[sqlx::test(migrations = "../../migrations")]
async fn create_then_instantiate_creates_monitor(pool: PgPool) {
    let r = common::router(pool);
    let c = register_admin(&r).await;

    // No monitors yet.
    let before: Vec<Value> = json(&r, Method::GET, "/v1/monitors", None, Some(&c)).await;
    assert!(before.is_empty());

    // Save a template.
    let t: Value = json(
        &r,
        Method::POST,
        "/v1/monitor-templates",
        Some(template_payload(
            "HTTP base",
            "templated monitor",
            "https://example.com",
        )),
        Some(&c),
    )
    .await;
    assert_eq!(t["name"], "HTTP base");
    let id = t["id"].as_str().unwrap();

    // Instantiate with a name override.
    let m: Value = json(
        &r,
        Method::POST,
        &format!("/v1/monitor-templates/{id}/instantiate"),
        Some(json!({ "name": "from template" })),
        Some(&c),
    )
    .await;
    assert_eq!(m["name"], "from template");
    assert_eq!(m["kind"], "http");

    // A new monitor now exists.
    let after: Vec<Value> = json(&r, Method::GET, "/v1/monitors", None, Some(&c)).await;
    assert_eq!(after.len(), 1);
    assert_eq!(after[0]["name"], "from template");
}

#[sqlx::test(migrations = "../../migrations")]
async fn instantiate_without_override_uses_spec_name(pool: PgPool) {
    let r = common::router(pool);
    let c = register_admin(&r).await;

    let t: Value = json(
        &r,
        Method::POST,
        "/v1/monitor-templates",
        Some(template_payload(
            "tpl",
            "spec default name",
            "https://x.example.com",
        )),
        Some(&c),
    )
    .await;
    let id = t["id"].as_str().unwrap();

    let m: Value = json(
        &r,
        Method::POST,
        &format!("/v1/monitor-templates/{id}/instantiate"),
        None,
        Some(&c),
    )
    .await;
    assert_eq!(m["name"], "spec default name");
}

#[sqlx::test(migrations = "../../migrations")]
async fn create_rejects_invalid_spec(pool: PgPool) {
    let r = common::router(pool);
    let c = register_admin(&r).await;

    let (status, _, _) = request(
        &r,
        Method::POST,
        "/v1/monitor-templates",
        Some(json!({
            "name": "bad",
            "spec": { "not": "a monitor" },
        })),
        Some(&c),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "../../migrations")]
async fn delete_removes_template(pool: PgPool) {
    let r = common::router(pool);
    let c = register_admin(&r).await;

    let t: Value = json(
        &r,
        Method::POST,
        "/v1/monitor-templates",
        Some(template_payload("gone", "m", "https://y.example.com")),
        Some(&c),
    )
    .await;
    let id = t["id"].as_str().unwrap();

    let (status, _, _) = request(
        &r,
        Method::DELETE,
        &format!("/v1/monitor-templates/{id}"),
        None,
        Some(&c),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _, _) = request(
        &r,
        Method::GET,
        &format!("/v1/monitor-templates/{id}"),
        None,
        Some(&c),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
