//! `/v1/notification-templates` integration tests.

mod common;

use axum::http::{Method, StatusCode};
use common::{json, register_admin, request};
use serde_json::{json, Value};
use sqlx::PgPool;

fn payload(name: &str) -> Value {
    json!({
        "name":             name,
        "event_kind":       "monitor_down",
        "channel_kinds":    ["slack", "webhook"],
        "subject_template": "[{{status}}] {{monitor.name}}",
        "body_template":    "{{monitor.name}} flipped to {{status}}",
    })
}

#[sqlx::test(migrations = "../../migrations")]
async fn list_empty_by_default(pool: PgPool) {
    let r = common::router(pool);
    let c = register_admin(&r).await;
    let ts: Vec<Value> = json(
        &r,
        Method::GET,
        "/v1/notification-templates",
        None,
        Some(&c),
    )
    .await;
    assert!(ts.is_empty());
}

#[sqlx::test(migrations = "../../migrations")]
async fn create_round_trips_all_fields(pool: PgPool) {
    let r = common::router(pool);
    let c = register_admin(&r).await;
    let t: Value = json(
        &r,
        Method::POST,
        "/v1/notification-templates",
        Some(payload("Concise")),
        Some(&c),
    )
    .await;
    assert_eq!(t["name"], "Concise");
    assert_eq!(t["event_kind"], "monitor_down");
    assert_eq!(t["channel_kinds"], json!(["slack", "webhook"]));

    let id = t["id"].as_str().unwrap();
    let g: Value = json(
        &r,
        Method::GET,
        &format!("/v1/notification-templates/{id}"),
        None,
        Some(&c),
    )
    .await;
    assert_eq!(g["name"], "Concise");
}

#[sqlx::test(migrations = "../../migrations")]
async fn patch_changes_body(pool: PgPool) {
    let r = common::router(pool);
    let c = register_admin(&r).await;
    let t: Value = json(
        &r,
        Method::POST,
        "/v1/notification-templates",
        Some(payload("Patched")),
        Some(&c),
    )
    .await;
    let id = t["id"].as_str().unwrap();
    let p: Value = json(
        &r,
        Method::PATCH,
        &format!("/v1/notification-templates/{id}"),
        Some(json!({ "body_template": "new body" })),
        Some(&c),
    )
    .await;
    assert_eq!(p["body_template"], "new body");
    // Name preserved.
    assert_eq!(p["name"], "Patched");
}

#[sqlx::test(migrations = "../../migrations")]
async fn delete_round_trip(pool: PgPool) {
    let r = common::router(pool);
    let c = register_admin(&r).await;
    let t: Value = json(
        &r,
        Method::POST,
        "/v1/notification-templates",
        Some(payload("Toast")),
        Some(&c),
    )
    .await;
    let id = t["id"].as_str().unwrap();
    let (s, _, _) = request(
        &r,
        Method::DELETE,
        &format!("/v1/notification-templates/{id}"),
        None,
        Some(&c),
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s2, _, _) = request(
        &r,
        Method::GET,
        &format!("/v1/notification-templates/{id}"),
        None,
        Some(&c),
    )
    .await;
    assert_eq!(s2, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../../migrations")]
async fn duplicate_name_returns_409(pool: PgPool) {
    let r = common::router(pool);
    let c = register_admin(&r).await;
    json::<Value>(
        &r,
        Method::POST,
        "/v1/notification-templates",
        Some(payload("DupName")),
        Some(&c),
    )
    .await;
    let (s, _, _) = request(
        &r,
        Method::POST,
        "/v1/notification-templates",
        Some(payload("DupName")),
        Some(&c),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
}

#[sqlx::test(migrations = "../../migrations")]
async fn empty_name_returns_400(pool: PgPool) {
    let r = common::router(pool);
    let c = register_admin(&r).await;
    let mut p = payload("ignored");
    p["name"] = json!("");
    let (s, _, _) = request(
        &r,
        Method::POST,
        "/v1/notification-templates",
        Some(p),
        Some(&c),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "../../migrations")]
async fn empty_body_returns_400(pool: PgPool) {
    let r = common::router(pool);
    let c = register_admin(&r).await;
    let mut p = payload("nobody");
    p["body_template"] = json!("");
    let (s, _, _) = request(
        &r,
        Method::POST,
        "/v1/notification-templates",
        Some(p),
        Some(&c),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}
