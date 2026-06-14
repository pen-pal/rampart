//! Trace read API (editor/readonly — these are views).
//!
//! /v1/traces              — recent traces (one row per trace_id)
//! /v1/traces/service-map  — service dependency edges
//! /v1/traces/{trace_id}   — all spans of a trace (the waterfall)

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use rampart_core::trace::{OperationStat, ServiceEdge, Span, TraceSummary};
use serde::Deserialize;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list))
        // static segments registered alongside the param route; axum 0.8
        // prefers the static match, so these resolve before `/{trace_id}`.
        .route("/service-map", get(service_map))
        .route("/operations", get(operations))
        .route("/{trace_id}", get(detail))
}

#[derive(Deserialize)]
struct ListQuery {
    limit: Option<i64>,
}

async fn list(
    State(s): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<TraceSummary>>, ApiError> {
    Ok(Json(
        rampart_db::traces::list_traces(s.pool(), q.limit.unwrap_or(100)).await?,
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

async fn detail(
    State(s): State<AppState>,
    Path(trace_id): Path<String>,
) -> Result<Json<Vec<Span>>, ApiError> {
    Ok(Json(
        rampart_db::traces::get_trace_spans(s.pool(), &trace_id).await?,
    ))
}
