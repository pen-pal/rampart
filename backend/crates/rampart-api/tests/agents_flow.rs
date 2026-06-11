//! Remote probe agents — end-to-end API flow (migration 0070).
//!
//! Covers both surfaces:
//!   - operator management (/v1/agents: RBAC, one-time token mint, revoke)
//!   - the agent wire protocol (/v1/agent/*: bearer auth, assignment pull,
//!     heartbeat ingestion with flip detection, cross-agent isolation)
//!
//! The test router runs without a scheduler, so ingestion exercises the
//! direct-insert fallback path — flip semantics (current_status bounce on
//! `important`) are identical by construction.

mod common;

use axum::body::{Body, Bytes};
use axum::http::{Method, Request, Response, StatusCode};
use axum::Router;
use common::{register_admin, request, user_with_role};
use http_body_util::BodyExt;
use rampart_core::Role;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;

fn monitor_body() -> Value {
    json!({
        "name": "Agent-probed",
        "kind": "http",
        "url": "https://example.com",
        "interval_seconds": 60
    })
}

/// Bearer-authenticated request for the agent wire protocol.
async fn agent_request(
    router: &Router,
    method: Method,
    path: &str,
    body_json: Option<Value>,
    token: &str,
) -> (StatusCode, Bytes) {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {token}"));
    if body_json.is_some() {
        req = req.header("content-type", "application/json");
    }
    let body = match body_json {
        Some(v) => Body::from(serde_json::to_vec(&v).unwrap()),
        None => Body::empty(),
    };
    let resp: Response<Body> = router
        .clone()
        .oneshot(req.body(body).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, bytes)
}

/// Mint an agent through the API and return (agent_id, raw_token).
async fn mint_agent(router: &Router, cookie: &str, name: &str) -> (String, String) {
    let (status, _, body) = request(
        router,
        Method::POST,
        "/v1/agents",
        Some(json!({ "name": name, "location": "eu-west" })),
        Some(cookie),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let v: Value = serde_json::from_slice(&body).unwrap();
    let token = v["token"].as_str().unwrap().to_string();
    assert!(
        token.starts_with("rmpa_"),
        "agent tokens carry the rmpa_ prefix, got {token}"
    );
    (v["agent"]["id"].as_str().unwrap().to_string(), token)
}

#[sqlx::test(migrations = "../../migrations")]
async fn admin_crud_rbac_and_one_time_token(pool: PgPool) {
    let router = common::router(pool.clone());
    let admin = register_admin(&router).await;

    let (agent_id, _token) = mint_agent(&router, &admin, "probe-1").await;

    // List hydrates count + liveness; a fresh agent has never polled.
    let (status, _, body) = request(&router, Method::GET, "/v1/agents", None, Some(&admin)).await;
    assert_eq!(status, StatusCode::OK);
    let agents: Vec<Value> = serde_json::from_slice(&body).unwrap();
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0]["online"], json!(false));
    assert_eq!(agents[0]["monitor_count"], json!(0));
    // The raw token never appears on reads.
    assert!(agents[0].get("token").is_none());

    // Editors can read the list (it feeds the assignment picker)…
    let editor = user_with_role(&pool, "editor@example.com", Role::Editor).await;
    let (status, _, _) = request(&router, Method::GET, "/v1/agents", None, Some(&editor)).await;
    assert_eq!(status, StatusCode::OK);
    // …but only admins manage agents.
    let (status, _, _) = request(
        &router,
        Method::POST,
        "/v1/agents",
        Some(json!({ "name": "nope" })),
        Some(&editor),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Rename via PATCH.
    let (status, _, body) = request(
        &router,
        Method::PATCH,
        &format!("/v1/agents/{agent_id}"),
        Some(json!({ "name": "probe-1-renamed" })),
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["name"], json!("probe-1-renamed"));

    // Revoke.
    let (status, _, _) = request(
        &router,
        Method::DELETE,
        &format!("/v1/agents/{agent_id}"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[sqlx::test(migrations = "../../migrations")]
async fn pull_report_flip_and_isolation(pool: PgPool) {
    let router = common::router(pool.clone());
    let admin = register_admin(&router).await;
    let (agent_id, token) = mint_agent(&router, &admin, "probe-1").await;
    let (_other_id, other_token) = mint_agent(&router, &admin, "probe-2").await;

    // Create a monitor assigned to probe-1, plus a local (unassigned) one.
    let mut body = monitor_body();
    body["agent_id"] = json!(agent_id);
    let (status, _, created) = request(
        &router,
        Method::POST,
        "/v1/monitors",
        Some(body),
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let assigned: Value = serde_json::from_slice(&created).unwrap();
    let monitor_id = assigned["id"].as_str().unwrap().to_string();
    assert_eq!(assigned["agent_id"], json!(agent_id));

    let (status, _, created) = request(
        &router,
        Method::POST,
        "/v1/monitors",
        Some(json!({ "name": "Local", "kind": "http", "url": "https://local.example.com" })),
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let local: Value = serde_json::from_slice(&created).unwrap();
    let local_id = local["id"].as_str().unwrap().to_string();

    // Pull: probe-1 sees exactly its own monitor.
    let (status, body) =
        agent_request(&router, Method::GET, "/v1/agent/monitors", None, &token).await;
    assert_eq!(status, StatusCode::OK);
    let pulled: Vec<Value> = serde_json::from_slice(&body).unwrap();
    assert_eq!(pulled.len(), 1);
    assert_eq!(pulled[0]["id"].as_str().unwrap(), monitor_id);

    // The pull bumped liveness — the dashboard badge flips online.
    let (_, _, body) = request(&router, Method::GET, "/v1/agents", None, Some(&admin)).await;
    let agents: Vec<Value> = serde_json::from_slice(&body).unwrap();
    let me = agents
        .iter()
        .find(|a| a["id"].as_str() == Some(agent_id.as_str()))
        .unwrap();
    assert_eq!(me["online"], json!(true));

    // Report Down → accepted, flips current_status (Pending → Down).
    let (status, body) = agent_request(
        &router,
        Method::POST,
        "/v1/agent/heartbeats",
        Some(json!([{
            "monitor_id": monitor_id,
            "status": "down",
            "latency_ms": 120,
            "msg": "connect timed out"
        }])),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let outcome: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(outcome["accepted"], json!(1));
    assert_eq!(outcome["rejected"].as_array().unwrap().len(), 0);

    let (_, _, body) = request(
        &router,
        Method::GET,
        &format!("/v1/monitors/{monitor_id}"),
        None,
        Some(&admin),
    )
    .await;
    let m: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(m["current_status"], json!("down"));

    // Recovery report flips it back up.
    let (_, body) = agent_request(
        &router,
        Method::POST,
        "/v1/agent/heartbeats",
        Some(json!([{ "monitor_id": monitor_id, "status": "up", "latency_ms": 80 }])),
        &token,
    )
    .await;
    let outcome: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(outcome["accepted"], json!(1));
    let (_, _, body) = request(
        &router,
        Method::GET,
        &format!("/v1/monitors/{monitor_id}"),
        None,
        Some(&admin),
    )
    .await;
    let m: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(m["current_status"], json!("up"));

    // Isolation: probe-2 may not report for probe-1's monitor, and nobody
    // reports for unassigned (local) monitors. Rejected per-row, not 4xx.
    let (status, body) = agent_request(
        &router,
        Method::POST,
        "/v1/agent/heartbeats",
        Some(json!([
            { "monitor_id": monitor_id, "status": "down" },
            { "monitor_id": local_id, "status": "down" }
        ])),
        &other_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let outcome: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(outcome["accepted"], json!(0));
    assert_eq!(outcome["rejected"].as_array().unwrap().len(), 2);

    // Garbage token → 401 on both endpoints.
    let (status, _) = agent_request(
        &router,
        Method::GET,
        "/v1/agent/monitors",
        None,
        "rmpa_bogus",
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "../../migrations")]
async fn push_monitors_cannot_be_agent_probed(pool: PgPool) {
    let router = common::router(pool);
    let admin = register_admin(&router).await;
    let (agent_id, _token) = mint_agent(&router, &admin, "probe-1").await;

    // Assignment at create is rejected.
    let (status, _, _) = request(
        &router,
        Method::POST,
        "/v1/monitors",
        Some(json!({ "name": "cron", "kind": "push", "agent_id": agent_id })),
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // …and at update.
    let (_, _, created) = request(
        &router,
        Method::POST,
        "/v1/monitors",
        Some(json!({ "name": "cron", "kind": "push" })),
        Some(&admin),
    )
    .await;
    let push: Value = serde_json::from_slice(&created).unwrap();
    let push_id = push["id"].as_str().unwrap();
    let (status, _, _) = request(
        &router,
        Method::PATCH,
        &format!("/v1/monitors/{push_id}"),
        Some(json!({ "agent_id": agent_id })),
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "../../migrations")]
async fn revoking_agent_returns_monitors_to_local(pool: PgPool) {
    let router = common::router(pool);
    let admin = register_admin(&router).await;
    let (agent_id, token) = mint_agent(&router, &admin, "probe-1").await;

    let mut body = monitor_body();
    body["agent_id"] = json!(agent_id);
    let (_, _, created) = request(
        &router,
        Method::POST,
        "/v1/monitors",
        Some(body),
        Some(&admin),
    )
    .await;
    let monitor: Value = serde_json::from_slice(&created).unwrap();
    let monitor_id = monitor["id"].as_str().unwrap();

    let (status, _, _) = request(
        &router,
        Method::DELETE,
        &format!("/v1/agents/{agent_id}"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Token is dead immediately…
    let (status, _) = agent_request(&router, Method::GET, "/v1/agent/monitors", None, &token).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // …and the monitor fell back to local probing (ON DELETE SET NULL).
    let (_, _, body) = request(
        &router,
        Method::GET,
        &format!("/v1/monitors/{monitor_id}"),
        None,
        Some(&admin),
    )
    .await;
    let m: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(m["agent_id"], json!(null));
}
