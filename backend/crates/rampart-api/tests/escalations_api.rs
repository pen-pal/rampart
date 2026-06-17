//! Escalation policy CRUD + episode acknowledge over the API.

mod common;

use axum::http::{Method, StatusCode};
use common::{register_admin, request, user_with_role};
use rampart_core::Role;
use serde_json::{json, Value};
use sqlx::PgPool;

fn ladder() -> Value {
    json!({
        "name": "ops ladder",
        "steps": [
            { "wait_seconds": 0, "channel_ids": [] },
        ]
    })
}

#[sqlx::test(migrations = "../../migrations")]
async fn policy_crud_and_validation(pool: PgPool) {
    let router = common::router(pool);
    let admin = register_admin(&router).await;

    // Step without channels is rejected.
    let (status, _, _) = request(
        &router,
        Method::POST,
        "/v1/escalation-policies",
        Some(ladder()),
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Valid ladder.
    let body = json!({
        "name": "ops ladder",
        "steps": [
            { "wait_seconds": 0,   "channel_ids": [uuid::Uuid::new_v4()] },
            { "wait_seconds": 600, "channel_ids": [uuid::Uuid::new_v4()] }
        ]
    });
    let (status, _, created) = request(
        &router,
        Method::POST,
        "/v1/escalation-policies",
        Some(body),
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let policy: Value = serde_json::from_slice(&created).unwrap();
    let pid = policy["id"].as_str().unwrap().to_string();

    // Step 1 with a wait is rejected on update too.
    let (status, _, _) = request(
        &router,
        Method::PATCH,
        &format!("/v1/escalation-policies/{pid}"),
        Some(json!({ "steps": [{ "wait_seconds": 60, "channel_ids": [uuid::Uuid::new_v4()] }] })),
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _, body) = request(
        &router,
        Method::GET,
        "/v1/escalation-policies",
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let policies: Vec<Value> = serde_json::from_slice(&body).unwrap();
    assert_eq!(policies.len(), 1);
    assert_eq!(policies[0]["monitor_count"], json!(0));

    let (status, _, _) = request(
        &router,
        Method::DELETE,
        &format!("/v1/escalation-policies/{pid}"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[sqlx::test(migrations = "../../migrations")]
async fn monitor_attach_episode_and_ack(pool: PgPool) {
    let router = common::router(pool.clone());
    let admin = register_admin(&router).await;

    let (_, _, created) = request(
        &router,
        Method::POST,
        "/v1/escalation-policies",
        Some(json!({
            "name": "ladder",
            "steps": [{ "wait_seconds": 0, "channel_ids": [uuid::Uuid::new_v4()] }]
        })),
        Some(&admin),
    )
    .await;
    let policy: Value = serde_json::from_slice(&created).unwrap();
    let pid = policy["id"].as_str().unwrap();

    // Attach at create; visible on the monitor.
    let (status, _, created) = request(
        &router,
        Method::POST,
        "/v1/monitors",
        Some(json!({
            "name": "web", "kind": "http", "url": "https://example.com",
            "escalation_policy_id": pid
        })),
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let monitor: Value = serde_json::from_slice(&created).unwrap();
    assert_eq!(monitor["escalation_policy_id"].as_str(), Some(pid));
    let mid = monitor["id"].as_str().unwrap().to_string();

    // No episode while healthy.
    let (status, _, body) = request(
        &router,
        Method::GET,
        &format!("/v1/monitors/{mid}/escalation"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(serde_json::from_slice::<Value>(&body).unwrap(), Value::Null);

    // Ack with nothing open → 404.
    let (status, _, _) = request(
        &router,
        Method::POST,
        &format!("/v1/monitors/{mid}/escalation/ack"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Open an episode through the db layer (the notifier does this in
    // prod; the test router runs without one) and ack it over the API.
    let policy_typed = rampart_db::escalations::get_unscoped(
        &pool,
        rampart_core::ids::EscalationPolicyId::from_uuid(pid.parse().unwrap()),
    )
    .await
    .unwrap();
    let monitor_id = rampart_core::MonitorId::from_uuid(mid.parse().unwrap());
    rampart_db::escalations::open_episode(&pool, monitor_id, &policy_typed)
        .await
        .unwrap()
        .unwrap();

    let (status, _, body) = request(
        &router,
        Method::GET,
        &format!("/v1/monitors/{mid}/escalation"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let episode: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(episode["last_step"], json!(0));
    assert!(episode["acked_at"].is_null());

    // Readonly may look but not ack.
    let readonly = user_with_role(&pool, "ro@example.com", Role::Readonly).await;
    let (status, _, _) = request(
        &router,
        Method::POST,
        &format!("/v1/monitors/{mid}/escalation/ack"),
        None,
        Some(&readonly),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _, body) = request(
        &router,
        Method::POST,
        &format!("/v1/monitors/{mid}/escalation/ack"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let acked: Value = serde_json::from_slice(&body).unwrap();
    assert!(!acked["acked_at"].is_null());
}
