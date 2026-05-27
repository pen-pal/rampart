//! `/v1/notifications` + attach/detach + template assignment tests.

mod common;

use axum::http::{Method, StatusCode};
use common::{json, register_admin, request};
use serde_json::{json, Value};
use sqlx::PgPool;

async fn create_webhook_channel(r: &axum::Router, c: &str, name: &str) -> String {
    let v: Value = json(r, Method::POST, "/v1/notifications", Some(json!({
        "kind":   "webhook",
        "name":   name,
        "config": { "url": "https://example.com/hook" },
        "active": true,
    })), Some(c)).await;
    v["id"].as_str().unwrap().to_string()
}

async fn create_http_monitor(r: &axum::Router, c: &str, name: &str) -> String {
    let v: Value = json(r, Method::POST, "/v1/monitors", Some(json!({
        "name": name, "kind": "http", "url": format!("https://{name}.example.com"),
        "interval_seconds": 60, "timeout_seconds": 10, "accepted_statuses": [200],
    })), Some(c)).await;
    v["id"].as_str().unwrap().to_string()
}

#[sqlx::test(migrations = "../../migrations")]
async fn channels_crud_round_trip(pool: PgPool) {
    let r = common::router(pool);
    let c = register_admin(&r).await;

    let ms: Vec<Value> = json(&r, Method::GET, "/v1/notifications", None, Some(&c)).await;
    assert!(ms.is_empty());

    let id = create_webhook_channel(&r, &c, "hook").await;
    let one: Value = json(&r, Method::GET, &format!("/v1/notifications/{id}"), None, Some(&c)).await;
    assert_eq!(one["name"], "hook");
    assert_eq!(one["kind"], "webhook");

    let (s, _, _) = request(&r, Method::DELETE, &format!("/v1/notifications/{id}"), None, Some(&c)).await;
    assert_eq!(s, StatusCode::NO_CONTENT);

    let ms: Vec<Value> = json(&r, Method::GET, "/v1/notifications", None, Some(&c)).await;
    assert!(ms.is_empty());
}

#[sqlx::test(migrations = "../../migrations")]
async fn patch_renames_and_toggles_active(pool: PgPool) {
    let r = common::router(pool);
    let c = register_admin(&r).await;
    let id = create_webhook_channel(&r, &c, "orig").await;

    let updated: Value = json(&r, Method::PATCH, &format!("/v1/notifications/{id}"),
        Some(json!({ "name": "renamed", "active": false })), Some(&c)).await;
    assert_eq!(updated["name"], "renamed");
    assert_eq!(updated["active"], false);
}

#[sqlx::test(migrations = "../../migrations")]
async fn attach_detach_via_api(pool: PgPool) {
    let r  = common::router(pool);
    let c  = register_admin(&r).await;
    let mid = create_http_monitor(&r, &c, "m").await;
    let nid = create_webhook_channel(&r, &c, "n").await;

    let (s1, _, _) = request(&r, Method::POST,
        &format!("/v1/monitors/{mid}/notifications/{nid}"), None, Some(&c)).await;
    assert_eq!(s1, StatusCode::NO_CONTENT);

    let attached: Vec<Value> = json(&r, Method::GET,
        &format!("/v1/monitors/{mid}/notifications"), None, Some(&c)).await;
    assert_eq!(attached.len(), 1);
    assert_eq!(attached[0]["id"], nid);

    let (s2, _, _) = request(&r, Method::DELETE,
        &format!("/v1/monitors/{mid}/notifications/{nid}"), None, Some(&c)).await;
    assert_eq!(s2, StatusCode::NO_CONTENT);
}

#[sqlx::test(migrations = "../../migrations")]
async fn counts_endpoint_reflects_attachments(pool: PgPool) {
    let r  = common::router(pool);
    let c  = register_admin(&r).await;
    let mid = create_http_monitor(&r, &c, "cm").await;
    let nid = create_webhook_channel(&r, &c, "cn").await;

    let pre: Vec<Value> = json(&r, Method::GET, "/v1/notifications/counts", None, Some(&c)).await;
    assert!(pre.is_empty());

    request(&r, Method::POST, &format!("/v1/monitors/{mid}/notifications/{nid}"),
        None, Some(&c)).await;
    let post: Vec<Value> = json(&r, Method::GET, "/v1/notifications/counts", None, Some(&c)).await;
    assert_eq!(post.len(), 1);
    assert_eq!(post[0]["monitor_id"], mid);
    assert_eq!(post[0]["count"],      1);
}

#[sqlx::test(migrations = "../../migrations")]
async fn template_assign_and_clear(pool: PgPool) {
    let r  = common::router(pool);
    let c  = register_admin(&r).await;
    let nid = create_webhook_channel(&r, &c, "tn").await;

    let t: Value = json(&r, Method::POST, "/v1/notification-templates",
        Some(json!({
            "name": "t1",
            "event_kind": "monitor_down",
            "body_template": "{{monitor.name}} is {{status}}",
        })), Some(&c)).await;
    let tid = t["id"].as_str().unwrap().to_string();

    // assign
    let updated: Value = json(&r, Method::PATCH, &format!("/v1/notifications/{nid}"),
        Some(json!({ "template_id": tid })), Some(&c)).await;
    assert_eq!(updated["template_id"], tid);

    // clear via explicit null
    let cleared: Value = json(&r, Method::PATCH, &format!("/v1/notifications/{nid}"),
        Some(json!({ "template_id": null })), Some(&c)).await;
    assert!(cleared["template_id"].is_null());
}

#[sqlx::test(migrations = "../../migrations")]
async fn rejects_unknown_channel_kind(pool: PgPool) {
    let r = common::router(pool);
    let c = register_admin(&r).await;
    let (s, _, _) = request(&r, Method::POST, "/v1/notifications", Some(json!({
        "kind": "not_a_real_kind", "name": "x", "config": {},
    })), Some(&c)).await;
    // serde rejects unknown enum variant → 422 from axum.
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
}
