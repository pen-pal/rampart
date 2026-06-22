//! Syslog + NDJSON log ingest: a forwarder POSTing RFC5424 / RFC3164 lines or
//! JSON-lines lands rows in the `logs` table, org-stamped from the ingest
//! credential (Default when token-less), and parsed into the standard log shape.

mod common;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use common::router;
use rampart_core::ids::OrgId;
use sqlx::PgPool;
use tower::ServiceExt;

const OTHER: &str = "00000000-0000-0000-0000-0000000c0ffe";
const DEFAULT_ORG: &str = "00000000-0000-0000-0000-000000000001";

async fn post(
    app: &axum::Router,
    uri: &str,
    token: Option<&str>,
    body: &'static str,
) -> StatusCode {
    let mut b = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header("content-type", "text/plain");
    if let Some(t) = token {
        b = b.header("x-rampart-token", t);
    }
    app.clone()
        .oneshot(b.body(Body::from(body)).unwrap())
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
async fn syslog_text_ingest_org_stamp_and_parse(pool: PgPool) {
    let app = router(pool.clone());
    sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1::uuid, 'other', 'Other') ON CONFLICT DO NOTHING")
        .bind(OTHER)
        .execute(&pool)
        .await
        .unwrap();
    let other = OrgId::from_uuid(uuid::Uuid::parse_str(OTHER).unwrap());
    let (_k, token) = rampart_db::ingest_keys::create(&pool, other, "fwd", "all", &[])
        .await
        .unwrap();

    // Two RFC5424 lines bearing the org key → both land in that org.
    let body = "<34>1 2003-10-11T22:14:15.003Z host su - - - first event\n<165>1 2003-10-11T22:14:16Z host app - - - second event";
    assert_eq!(
        post(&app, "/syslog", Some(&token), body).await,
        StatusCode::OK
    );
    assert_eq!(
        count_logs(&pool, OTHER).await,
        2,
        "both lines stamped to key org"
    );
    assert_eq!(count_logs(&pool, DEFAULT_ORG).await, 0, "nothing misfiled");

    // The parsed row carries service + severity + source attribute. Select the
    // specific line by body (the two batch rows share received_at, so ordering
    // isn't deterministic — key on content instead).
    let row: (String, i16, serde_json::Value) = sqlx::query_as(
        "SELECT service_name, severity, attributes FROM logs WHERE org_id = $1::uuid AND body = 'first event'",
    )
    .bind(OTHER)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.0, "su");
    assert_eq!(row.1, 19); // crit -> ERROR
    assert_eq!(row.2["log.source"], "syslog");

    // Token-less open ingest → Default org.
    assert_eq!(
        post(
            &app,
            "/syslog",
            None,
            "<13>1 2003-10-11T22:14:17Z host app - - - tokenless"
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        count_logs(&pool, DEFAULT_ORG).await,
        1,
        "tokenless → Default"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn syslog_json_ingest(pool: PgPool) {
    let app = router(pool.clone());
    let body = r#"{"level":"error","message":"db down","service":"api"}
{"level":"info","message":"recovered","service":"api"}"#;
    assert_eq!(post(&app, "/syslog/json", None, body).await, StatusCode::OK);
    assert_eq!(count_logs(&pool, DEFAULT_ORG).await, 2, "two NDJSON lines");

    let sev: i16 = sqlx::query_scalar(
        "SELECT severity FROM logs WHERE org_id = $1::uuid AND body = 'db down'",
    )
    .bind(DEFAULT_ORG)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(sev, 17, "error -> ERROR severity");
}

#[sqlx::test(migrations = "../../migrations")]
async fn syslog_skips_malformed_lines(pool: PgPool) {
    let app = router(pool.clone());
    // One valid line between two junk lines → exactly one row.
    let body = "garbage line\n<34>1 2003-10-11T22:14:15Z host app - - - ok\nmore junk";
    assert_eq!(post(&app, "/syslog", None, body).await, StatusCode::OK);
    assert_eq!(
        count_logs(&pool, DEFAULT_ORG).await,
        1,
        "only the valid line stored"
    );
}
