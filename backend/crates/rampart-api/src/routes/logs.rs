//! Log read API (editor/readonly — a view).
//!
//! /v1/logs           — recent logs, filtered (service / level / search / trace)
//! /v1/logs/services  — distinct service names for the filter dropdown
//!
//! Ingest (OTLP) lives in `otlp`, mounted at the root `/otlp` surface.

use crate::csv::csv_escape;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use rampart_core::log::level_min_severity;
use rampart_core::LogEntry;
use rampart_db::logs::LogFilter;
use serde::Deserialize;

/// CSV export is capped to bound peak memory (the rows are materialised in a
/// single String). Routine analysis / SIEM hand-off fits well within this;
/// continuous forwarding should use the syslog/SIEM export sink instead.
const EXPORT_CAP: i64 = 50_000;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list))
        .route("/services", get(services))
        .route("/levels", get(levels))
        .route("/histogram", get(histogram))
        .route("/export.csv", get(export_csv))
}

#[derive(Deserialize)]
struct LevelsQuery {
    service: Option<String>,
    hours: Option<i32>,
}

async fn levels(
    State(s): State<AppState>,
    Query(q): Query<LevelsQuery>,
) -> Result<Json<Vec<(String, i64)>>, ApiError> {
    Ok(Json(
        rampart_db::logs::level_counts(
            s.pool(),
            q.service.as_deref().filter(|s| !s.is_empty()),
            q.hours.unwrap_or(24),
        )
        .await?,
    ))
}

#[derive(Deserialize)]
struct LogQuery {
    service: Option<String>,
    /// Minimum coarse level: trace | debug | info | warn | error | fatal.
    level: Option<String>,
    /// Case-insensitive substring match on the body.
    q: Option<String>,
    trace_id: Option<String>,
    span_id: Option<String>,
    limit: Option<i64>,
    hours: Option<i32>,
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
        span_id: query.span_id.as_deref().filter(|s| !s.is_empty()),
        limit: query.limit.unwrap_or(200),
    };
    Ok(Json(rampart_db::logs::query_logs(s.pool(), filter).await?))
}

async fn histogram(
    State(s): State<AppState>,
    Query(query): Query<LogQuery>,
) -> Result<Json<Vec<rampart_db::logs::LogBucket>>, ApiError> {
    let min_severity = query.level.as_deref().and_then(level_min_severity);
    Ok(Json(
        rampart_db::logs::histogram(
            s.pool(),
            query.service.as_deref().filter(|s| !s.is_empty()),
            min_severity,
            query.q.as_deref().filter(|s| !s.is_empty()),
            query.hours.unwrap_or(24),
            48,
        )
        .await?,
    ))
}

async fn services(State(s): State<AppState>) -> Result<Json<Vec<String>>, ApiError> {
    Ok(Json(rampart_db::logs::list_services(s.pool()).await?))
}

/// CSV export of logs honouring the same `service` / `level` / `q` / `trace_id`
/// filters as the list route. `limit` is accepted but clamped to [`EXPORT_CAP`].
async fn export_csv(
    State(s): State<AppState>,
    Query(query): Query<LogQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let min_severity = query.level.as_deref().and_then(level_min_severity);
    let limit = query.limit.unwrap_or(EXPORT_CAP).clamp(1, EXPORT_CAP);
    let filter = LogFilter {
        service: query.service.as_deref(),
        min_severity,
        query: query.q.as_deref().filter(|s| !s.is_empty()),
        trace_id: query.trace_id.as_deref(),
        span_id: query.span_id.as_deref().filter(|s| !s.is_empty()),
        limit,
    };
    let rows = rampart_db::logs::query_logs(s.pool(), filter).await?;
    let fmt = time::format_description::well_known::Rfc3339;
    let mut body = String::with_capacity(64 + rows.len() * 160);
    body.push_str("ts,level,severity,service,trace_id,span_id,body,attributes\n");
    for r in &rows {
        body.push_str(&format!(
            "{},{},{},{},{},{},{},{}\n",
            r.ts.format(&fmt).unwrap_or_default(),
            csv_escape(&r.level),
            r.severity,
            csv_escape(&r.service_name),
            csv_escape(r.trace_id.as_deref().unwrap_or("")),
            csv_escape(r.span_id.as_deref().unwrap_or("")),
            csv_escape(&r.body),
            csv_escape(
                &r.attributes
                    .as_ref()
                    .map(|a| a.to_string())
                    .unwrap_or_default(),
            ),
        ));
    }
    Ok((
        [
            ("content-type", "text/csv; charset=utf-8".to_string()),
            (
                "content-disposition",
                "attachment; filename=\"logs.csv\"".to_string(),
            ),
        ],
        body,
    ))
}
