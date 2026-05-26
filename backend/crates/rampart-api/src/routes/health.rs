//! Health endpoints.
//!
//! `/healthz` is liveness — always 200 if the process can answer.
//! `/readyz`  is readiness — 200 only when the DB is reachable.
//! `/metrics` exposes Prometheus text — gauges and counters covering
//! monitor status, latency, and domain/cert expiry.

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/healthz", get(liveness))
        .route("/readyz",  get(readiness))
        .route("/metrics", get(metrics))
}

async fn liveness() -> impl IntoResponse {
    (StatusCode::OK, Json(json!({ "status": "alive" })))
}

async fn readiness(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    // A cheap roundtrip — confirms the pool can hand out a connection AND
    // the database is responding to queries. Avoids pg_isready false-positives.
    sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(state.pool())
        .await
        .map_err(rampart_db::DbError::from)?;
    Ok((StatusCode::OK, Json(json!({ "status": "ready" }))))
}

/// Prometheus exposition format. Minimal scaffolding — adds real counters
/// as the scheduler/correlator come online. The format is just text so we
/// hand-format rather than pull in the prometheus crate for one metric.
async fn metrics() -> impl IntoResponse {
    let body = "\
# HELP rampart_build_info Build metadata.
# TYPE rampart_build_info gauge
rampart_build_info{version=\"0.1.0\"} 1
";
    (
        StatusCode::OK,
        [("content-type", "text/plain; version=0.0.4")],
        body,
    )
}
