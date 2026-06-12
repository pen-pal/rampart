//! Log read API (editor/readonly — a view).
//!
//! /v1/logs           — recent logs, filtered (service / level / search / trace)
//! /v1/logs/services  — distinct service names for the filter dropdown
//!
//! Ingest (OTLP) lives in `otlp`, mounted at the root `/otlp` surface.

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use rampart_core::log::level_min_severity;
use rampart_core::LogEntry;
use rampart_db::logs::LogFilter;
use serde::Deserialize;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list))
        .route("/services", get(services))
}

#[derive(Deserialize)]
struct LogQuery {
    service: Option<String>,
    /// Minimum coarse level: trace | debug | info | warn | error | fatal.
    level: Option<String>,
    /// Case-insensitive substring match on the body.
    q: Option<String>,
    trace_id: Option<String>,
    limit: Option<i64>,
}

async fn list(
    State(s): State<AppState>,
    Query(query): Query<LogQuery>,
) -> Result<Json<Vec<LogEntry>>, ApiError> {
    let min_severity = query.level.as_deref().and_then(level_min_severity);
    let filter = LogFilter {
        service: query.service.as_deref(),
        min_severity,
        query: query.q.as_deref().filter(|s| !s.is_empty()),
        trace_id: query.trace_id.as_deref(),
        limit: query.limit.unwrap_or(200),
    };
    Ok(Json(rampart_db::logs::query_logs(s.pool(), filter).await?))
}

async fn services(State(s): State<AppState>) -> Result<Json<Vec<String>>, ApiError> {
    Ok(Json(rampart_db::logs::list_services(s.pool()).await?))
}
