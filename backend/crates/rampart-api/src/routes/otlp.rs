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
    // Phase 5: resolve the owning org from the ingest credential (gates auth
    // internally, exactly as require_telemetry_token did; Default on key-miss).
    let org = crate::ingest_util::resolve_ingest_org(s.pool(), &headers, None).await?;
    // RLS: bind the resolved org so the pool hook scopes the writes below to
    // this tenant (no-op when RAMPART_RLS off). Ingest handlers carry no
    // session, so the chokepoint is here rather than the auth middleware.
    rampart_db::rls::with_org(org, async move {
        let content_type = headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        // OTel SDKs/Collectors gzip OTLP/HTTP exports by default — inflate first.
        let body = crate::ingest_util::decompress(&headers, &body)?;

        let mut logs: Vec<ParsedLog> = if content_type.contains("protobuf") {
            crate::otlp_proto::parse_otlp_logs_protobuf(&body)
                .map_err(|e| ApiError::BadRequest(format!("invalid OTLP protobuf: {e}")))?
        } else {
            let v: Value = serde_json::from_slice(&body)
                .map_err(|_| ApiError::BadRequest("invalid OTLP JSON body".into()))?;
            rampart_core::log::parse_otlp_logs_json(&v)
        };
        crate::ingest_util::enforce_record_cap(logs.len())?;

        // Head sampling. A log carrying a trace_id is sampled on that id (so it
        // follows its trace when both rates match); a trace-less log falls back to
        // a per-record key (service + time + body) for a uniform spread.
        let sc = crate::ingest_util::sampling_config(s.pool()).await?;
        if sc.logs_pct < 100 {
            logs.retain(|l| {
                let key = l
                    .trace_id
                    .clone()
                    .unwrap_or_else(|| format!("{}:{}:{}", l.service_name, l.time_ns, l.body));
                rampart_core::sampling::keep(&key, sc.logs_pct)
            });
        }

        s.store().insert_logs(&logs, org).await?;
        Ok(Json(serde_json::json!({})))
    })
    .await
}

async fn ingest_traces(
    State(s): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    // Phase 5: resolve the owning org from the ingest credential (gates auth
    // internally, exactly as require_telemetry_token did; Default on key-miss).
    let org = crate::ingest_util::resolve_ingest_org(s.pool(), &headers, None).await?;
    // RLS: bind the resolved org so the writes below are tenant-scoped under
    // the pool hook (no-op when RAMPART_RLS off).
    rampart_db::rls::with_org(org, async move {
        let content_type = headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        // OTel SDKs/Collectors gzip OTLP/HTTP exports by default — inflate first.
        let body = crate::ingest_util::decompress(&headers, &body)?;

        let mut spans: Vec<ParsedSpan> = if content_type.contains("protobuf") {
            crate::otlp_proto::parse_otlp_traces_protobuf(&body)
                .map_err(|e| ApiError::BadRequest(format!("invalid OTLP protobuf: {e}")))?
        } else {
            let v: Value = serde_json::from_slice(&body)
                .map_err(|_| ApiError::BadRequest("invalid OTLP JSON body".into()))?;
            rampart_core::trace::parse_otlp_traces_json(&v)
        };
        crate::ingest_util::enforce_record_cap(spans.len())?;

        // Head sampling, keyed on trace_id so every span of a kept trace survives
        // and a dropped trace leaves no orphans (consistent across batches/replicas).
        let sc = crate::ingest_util::sampling_config(s.pool()).await?;
        if sc.traces_pct < 100 {
            spans.retain(|sp| rampart_core::sampling::keep(&sp.trace_id, sc.traces_pct));
        }

        s.store().insert_spans(&spans, org).await?;

        // OTLP ExportTraceServiceResponse — an empty object signals full success.
        Ok(Json(serde_json::json!({})))
    })
    .await
}
