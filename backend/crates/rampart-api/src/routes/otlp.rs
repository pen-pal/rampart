//! OTLP trace ingest (OpenTelemetry Protocol over HTTP).
//!
//! `POST /otlp/v1/traces` — accepts an `ExportTraceServiceRequest` as OTLP/JSON
//! (`application/json`) or OTLP/protobuf (`application/x-protobuf`). Mounted at
//! the root `/otlp` surface outside the session layer: in a single-tenant
//! self-host deployment the operator controls network exposure (like a
//! Prometheus scrape target). Point an OTel SDK/Collector's OTLP/HTTP exporter
//! at `http://<host>/otlp` (it appends `/v1/traces`).

use crate::error::ApiError;
use crate::state::AppState;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};
use rampart_core::log::ParsedLog;
use rampart_core::trace::ParsedSpan;
use serde_json::Value;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/traces", post(ingest_traces))
        .route("/v1/logs", post(ingest_logs))
        // OTLP profiling signal (v1development). The handler lives in the
        // profiles module; mounted here so it sits on the same /otlp surface
        // an OTLP/HTTP exporter targets.
        .route(
            "/v1development/profiles",
            post(crate::routes::profiles::ingest_otlp),
        )
}

async fn ingest_logs(
    State(s): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    crate::ingest_util::require_telemetry_token(s.pool(), &headers, None).await?;
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // OTel SDKs/Collectors gzip OTLP/HTTP exports by default — inflate first.
    let body = crate::ingest_util::decompress(&headers, &body)?;

    let logs: Vec<ParsedLog> = if content_type.contains("protobuf") {
        crate::otlp_proto::parse_otlp_logs_protobuf(&body)
            .map_err(|e| ApiError::BadRequest(format!("invalid OTLP protobuf: {e}")))?
    } else {
        let v: Value = serde_json::from_slice(&body)
            .map_err(|_| ApiError::BadRequest("invalid OTLP JSON body".into()))?;
        rampart_core::log::parse_otlp_logs_json(&v)
    };

    rampart_db::logs::insert_logs(s.pool(), &logs).await?;
    Ok(Json(serde_json::json!({})))
}

async fn ingest_traces(
    State(s): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    crate::ingest_util::require_telemetry_token(s.pool(), &headers, None).await?;
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // OTel SDKs/Collectors gzip OTLP/HTTP exports by default — inflate first.
    let body = crate::ingest_util::decompress(&headers, &body)?;

    let spans: Vec<ParsedSpan> = if content_type.contains("protobuf") {
        crate::otlp_proto::parse_otlp_traces_protobuf(&body)
            .map_err(|e| ApiError::BadRequest(format!("invalid OTLP protobuf: {e}")))?
    } else {
        let v: Value = serde_json::from_slice(&body)
            .map_err(|_| ApiError::BadRequest("invalid OTLP JSON body".into()))?;
        rampart_core::trace::parse_otlp_traces_json(&v)
    };

    rampart_db::traces::insert_spans(s.pool(), &spans).await?;

    // OTLP ExportTraceServiceResponse — an empty object signals full success.
    Ok(Json(serde_json::json!({})))
}
