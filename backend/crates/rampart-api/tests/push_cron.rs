//! Cron-job run states on push monitors (migration 0071).
//!
//! /push/:token/run|complete|fail (and the ?state= form): run opens a
//! duration clock without recording a heartbeat, complete/fail close it,
//! the duration lands in latency_ms, and a fail flips the monitor Down
//! immediately through the shared external-ingest path — no scheduler
//! tick involved (the test router runs without one).

mod common;

use axum::http::{Method, StatusCode};
use common::{register_admin, request};
use serde_json::{json, Value};
use sqlx::PgPool;

/// Create a push monitor and return (monitor_id, push_token).
async fn push_monitor(router: &axum::Router, cookie: &str) -> (String, String) {
    let (status, _, body) = request(
        router,
        Method::POST,
        "/v1/monitors",
        Some(json!({ "name": "nightly-backup", "kind": "push", "interval_seconds": 60 })),
        Some(cookie),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let v: Value = serde_json::from_slice(&body).unwrap();
    (
        v["id"].as_str().unwrap().to_string(),
        v["push_token"].as_str().unwrap().to_string(),
    )
}

async fn get_monitor(router: &axum::Router, cookie: &str, id: &str) -> Value {
    let (status, _, body) = request(
        router,
        Method::GET,
        &format!("/v1/monitors/{id}"),
        None,
        Some(cookie),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    serde_json::from_slice(&body).unwrap()
}

async fn heartbeats(router: &axum::Router, cookie: &str, id: &str) -> Vec<Value> {
    let (status, _, body) = request(
        router,
        Method::GET,
        &format!("/v1/monitors/{id}/heartbeats"),
        None,
        Some(cookie),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    serde_json::from_slice(&body).unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn run_complete_records_duration(pool: PgPool) {
    let router = common::router(pool);
    let admin = register_admin(&router).await;
    let (id, token) = push_monitor(&router, &admin).await;

    // run: opens the clock, records NO heartbeat.
    let (status, _, _) = request(
        &router,
        Method::POST,
        &format!("/push/{token}/run"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let m = get_monitor(&router, &admin, &id).await;
    assert!(
        !m["last_run_started_at"].is_null(),
        "run ping opens a run, got: {:?}",
        m["last_run_started_at"]
    );
    assert_eq!(heartbeats(&router, &admin, &id).await.len(), 0);

    // complete: closes the run, duration lands in latency_ms.
    let (status, _, _) = request(
        &router,
        Method::POST,
        &format!("/push/{token}/complete"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let m = get_monitor(&router, &admin, &id).await;
    assert_eq!(m["current_status"], json!("up"));
    assert!(
        m["last_run_started_at"].is_null(),
        "complete closes the run"
    );
    assert!(!m["last_push_at"].is_null(), "complete stamps liveness");

    let hbs = heartbeats(&router, &admin, &id).await;
    assert_eq!(hbs.len(), 1);
    assert_eq!(hbs[0]["status"], json!("up"));
    assert_eq!(hbs[0]["msg"], json!("run complete"));
    let dur = hbs[0]["latency_ms"].as_i64().expect("duration recorded");
    assert!((0..60_000).contains(&dur), "sane duration, got {dur}");
}

#[sqlx::test(migrations = "../../migrations")]
async fn fail_flips_down_immediately_and_recovers(pool: PgPool) {
    let router = common::router(pool);
    let admin = register_admin(&router).await;
    let (id, token) = push_monitor(&router, &admin).await;

    // fail (query-param form, with a custom message).
    let (status, _, _) = request(
        &router,
        Method::POST,
        &format!("/push/{token}?state=fail&msg=exit+code+1"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let m = get_monitor(&router, &admin, &id).await;
    assert_eq!(m["current_status"], json!("down"));
    let hbs = heartbeats(&router, &admin, &id).await;
    assert_eq!(hbs[0]["msg"], json!("exit code 1"));
    // The flip is marked important — that's what drives notifications.
    assert_eq!(hbs[0]["important"], json!(true));

    // complete → back Up, also important (a real recovery flip).
    let (status, _, _) = request(
        &router,
        Method::POST,
        &format!("/push/{token}/complete"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let m = get_monitor(&router, &admin, &id).await;
    assert_eq!(m["current_status"], json!("up"));
    let hbs = heartbeats(&router, &admin, &id).await;
    assert_eq!(hbs[0]["status"], json!("up"));
    assert_eq!(hbs[0]["important"], json!(true));
}

#[sqlx::test(migrations = "../../migrations")]
async fn legacy_status_pings_still_work(pool: PgPool) {
    let router = common::router(pool);
    let admin = register_admin(&router).await;
    let (id, token) = push_monitor(&router, &admin).await;

    // Bare ping defaults to up (≙ complete).
    let (status, _, _) =
        request(&router, Method::POST, &format!("/push/{token}"), None, None).await;
    assert_eq!(status, StatusCode::OK);
    let m = get_monitor(&router, &admin, &id).await;
    assert_eq!(m["current_status"], json!("up"));

    // ?status=down ≙ fail.
    let (status, _, _) = request(
        &router,
        Method::POST,
        &format!("/push/{token}?status=down&ping=42"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let m = get_monitor(&router, &admin, &id).await;
    assert_eq!(m["current_status"], json!("down"));
    // Explicit ?ping wins over a computed duration.
    let hbs = heartbeats(&router, &admin, &id).await;
    assert_eq!(hbs[0]["latency_ms"], json!(42));

    // GET works too (curl-in-crontab path), and unknown vocab is a 400.
    let (status, _, _) = request(&router, Method::GET, &format!("/push/{token}"), None, None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, _) = request(
        &router,
        Method::POST,
        &format!("/push/{token}/bogus"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _, _) = request(
        &router,
        Method::POST,
        &format!("/push/{token}?status=sideways"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Unknown token stays a 404.
    let (status, _, _) = request(
        &router,
        Method::POST,
        "/push/nope_not_a_token/complete",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../../migrations")]
async fn cron_config_accepted_on_push_monitor(pool: PgPool) {
    let router = common::router(pool);
    let admin = register_admin(&router).await;

    // The schedule lives in freeform config — verify it round-trips and
    // the parser accepts what the wizard will write.
    let (status, _, body) = request(
        &router,
        Method::POST,
        "/v1/monitors",
        Some(json!({
            "name": "hourly-etl",
            "kind": "push",
            "config": { "cron": "0 * * * *", "cron_grace_seconds": 120, "max_run_seconds": 900 }
        })),
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let m: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(m["config"]["cron"], json!("0 * * * *"));
    assert!(
        rampart_core::CronSchedule::from_config(&m["config"]).is_some(),
        "wizard-written config parses"
    );
}
