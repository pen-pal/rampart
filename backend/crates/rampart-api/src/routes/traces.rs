//! Trace read API (editor/readonly — these are views).
//!
//! /v1/traces              — recent traces (one row per trace_id)
//! /v1/traces/service-map  — service dependency edges
//! /v1/traces/{trace_id}   — all spans of a trace (the waterfall)

use crate::csv::csv_escape;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use rampart_core::trace::{OperationStat, ServiceEdge, Span, TraceSummary};
use serde::Deserialize;

/// CSV export cap — rows are materialised in a single String, so this bounds
/// peak memory. Mirrors the logs export cap.
const EXPORT_CAP: i64 = 50_000;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list))
        // static segments registered alongside the param route; axum 0.8
        // prefers the static match, so these resolve before `/{trace_id}`.
        .route("/service-map", get(service_map))
        .route("/operations", get(operations))
        .route("/operation-trend", get(operation_trend))
        .route("/export.csv", get(export_csv))
        .route("/{trace_id}", get(detail))
}

#[derive(Deserialize)]
struct ListQuery {
    limit: Option<i64>,
    service: Option<String>,
    min_duration_ms: Option<f64>,
    #[serde(default)]
    errors_only: bool,
    q: Option<String>,
}

async fn list(
    State(s): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<TraceSummary>>, ApiError> {
    let ne = |o: &Option<String>| o.clone().filter(|x| !x.is_empty());
    let service = ne(&query.service);
    let q = ne(&query.q);
    let filter = rampart_db::traces::TraceFilter {
        service: service.as_deref(),
        min_duration_ms: query.min_duration_ms,
        errors_only: query.errors_only,
        q: q.as_deref(),
        limit: query.limit.unwrap_or(100),
    };
    Ok(Json(
        rampart_db::traces::list_traces(s.pool(), filter).await?,
    ))
}

#[derive(Deserialize)]
struct MapQuery {
    hours: Option<i64>,
}

async fn service_map(
    State(s): State<AppState>,
    Query(q): Query<MapQuery>,
) -> Result<Json<Vec<ServiceEdge>>, ApiError> {
    Ok(Json(
        rampart_db::traces::service_map(s.pool(), q.hours.unwrap_or(24)).await?,
    ))
}

#[derive(Deserialize)]
struct OpsQuery {
    /// Optional service filter; empty/absent = all services.
    service: Option<String>,
    hours: Option<i64>,
}

async fn operations(
    State(s): State<AppState>,
    Query(q): Query<OpsQuery>,
) -> Result<Json<Vec<OperationStat>>, ApiError> {
    Ok(Json(
        rampart_db::traces::operation_stats(
            s.pool(),
            q.service.as_deref().unwrap_or(""),
            q.hours.unwrap_or(24),
        )
        .await?,
    ))
}

#[derive(serde::Deserialize)]
struct OpTrendQuery {
    service: String,
    operation: String,
    hours: Option<i64>,
}

async fn operation_trend(
    State(s): State<AppState>,
    Query(q): Query<OpTrendQuery>,
) -> Result<Json<Vec<f64>>, ApiError> {
    Ok(Json(
        rampart_db::traces::operation_trend(
            s.pool(),
            &q.service,
            &q.operation,
            q.hours.unwrap_or(24),
            24,
        )
        .await?,
    ))
}

/// CSV export of trace summaries honouring the same `service` /
/// `min_duration_ms` / `errors_only` / `q` filters as the list route. One row
/// per trace; `limit` is accepted but clamped to [`EXPORT_CAP`].
async fn export_csv(
    State(s): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let ne = |o: &Option<String>| o.clone().filter(|x| !x.is_empty());
    let service = ne(&query.service);
    let q = ne(&query.q);
    let limit = query.limit.unwrap_or(EXPORT_CAP).clamp(1, EXPORT_CAP);
    let filter = rampart_db::traces::TraceFilter {
        service: service.as_deref(),
        min_duration_ms: query.min_duration_ms,
        errors_only: query.errors_only,
        q: q.as_deref(),
        limit,
    };
    let rows = rampart_db::traces::list_traces(s.pool(), filter).await?;
    let fmt = time::format_description::well_known::Rfc3339;
    let mut body = String::with_capacity(64 + rows.len() * 120);
    body.push_str("started_at,trace_id,root_service,root_name,duration_ms,span_count,error_count,services\n");
    for r in &rows {
        body.push_str(&format!(
            "{},{},{},{},{},{},{},{}\n",
            r.started_at.format(&fmt).unwrap_or_default(),
            csv_escape(&r.trace_id),
            csv_escape(&r.root_service),
            csv_escape(&r.root_name),
            r.duration_ms,
            r.span_count,
            r.error_count,
            csv_escape(&r.services.join(";")),
        ));
    }
    Ok((
        [
            ("content-type", "text/csv; charset=utf-8".to_string()),
            (
                "content-disposition",
                "attachment; filename=\"traces.csv\"".to_string(),
            ),
        ],
        body,
    ))
}

async fn detail(
    State(s): State<AppState>,
    Path(trace_id): Path<String>,
) -> Result<Json<Vec<Span>>, ApiError> {
    Ok(Json(
        rampart_db::traces::get_trace_spans(s.pool(), &trace_id).await?,
    ))
}
