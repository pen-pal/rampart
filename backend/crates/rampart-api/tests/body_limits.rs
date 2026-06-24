//! Request-body caps differ by surface: the management/`/v1` routes cap at
//! `MAX_BODY_DEFAULT` (2 MiB) while the public telemetry-ingest router caps at
//! the much larger `MAX_BODY_INGEST` — so legitimate large OTLP / Prometheus /
//! profiles batches aren't silently 413-dropped (audit #123 rank-7).

mod common;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use sqlx::PgPool;
use tower::ServiceExt;

/// POST `len` bytes (declared via Content-Length, so the limit layer can reject
/// up front) and return the response status.
async fn post_bytes(router: &axum::Router, path: &str, len: usize) -> StatusCode {
    let req = Request::builder()
        .method(Method::POST)
        .uri(path)
        .header("content-type", "application/octet-stream")
        .header("content-length", len.to_string())
        .body(Body::from(vec![b'x'; len]))
        .unwrap();
    router.clone().oneshot(req).await.unwrap().status()
}

#[sqlx::test(migrations = "../../migrations")]
async fn default_surface_rejects_oversize_body(pool: PgPool) {
    let router = common::router(pool);
    // 3 MiB > the 2 MiB default cap → 413, and the cap is layered outside auth
    // so it trips before `require_session` would 401.
    let status = post_bytes(&router, "/v1/monitors", 3 * 1024 * 1024).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[sqlx::test(migrations = "../../migrations")]
async fn ingest_surface_allows_large_body(pool: PgPool) {
    let router = common::router(pool);
    // 3 MiB is comfortably under the ingest cap → NOT 413. The request still
    // fails downstream (it isn't valid snappy/protobuf), but the body cap is no
    // longer what drops it — which is the whole point of the per-surface limit.
    let status = post_bytes(&router, "/prom/write", 3 * 1024 * 1024).await;
    assert_ne!(
        status,
        StatusCode::PAYLOAD_TOO_LARGE,
        "ingest must accept payloads larger than the 2 MiB management cap"
    );
}
