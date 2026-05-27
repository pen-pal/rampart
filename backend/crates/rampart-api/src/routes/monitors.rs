//! `/v1/monitors` routes.
//!
//! Single-tenant: no workspace scoping. Authentication is a TODO —
//! the scaffold passes through. Add a session/JWT extractor before
//! exposing this to the internet.

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use rampart_core::monitor::{NewMonitor, UpdateMonitor};
use rampart_core::{Heartbeat, Monitor, MonitorId, MonitorStatus};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use time::OffsetDateTime;
use uuid::Uuid;
use validator::Validate;

pub fn router() -> Router<AppState> {
    // Static segments must be declared before the `:id` route so axum
    // matches them before treating the segment as an id.
    Router::new()
        .route("/", get(list).post(create))
        .route("/summary", get(summary))
        .route("/history", get(history_all))
        .route("/:id", get(get_one).patch(update).delete(delete_one))
        .route("/:id/heartbeats", get(heartbeats))
        .route("/:id/pause", post(pause))
        .route("/:id/resume", post(resume))
}

fn parse_monitor_id(s: &str) -> Result<MonitorId, ApiError> {
    Uuid::from_str(s)
        .map(MonitorId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid monitor id".into()))
}

async fn list(State(state): State<AppState>) -> Result<Json<Vec<Monitor>>, ApiError> {
    let monitors = rampart_db::monitors::list(state.pool()).await?;
    Ok(Json(monitors))
}

async fn create(
    State(state): State<AppState>,
    Json(input): Json<NewMonitor>,
) -> Result<(StatusCode, Json<Monitor>), ApiError> {
    input.validate()?;
    let monitor = rampart_db::monitors::create(state.pool(), input).await?;
    state.poke_scheduler();
    Ok((StatusCode::CREATED, Json(monitor)))
}

async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Monitor>, ApiError> {
    let monitor_id = parse_monitor_id(&id)?;
    let monitor = rampart_db::monitors::get(state.pool(), monitor_id).await?;
    Ok(Json(monitor))
}

async fn delete_one(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let monitor_id = parse_monitor_id(&id)?;
    rampart_db::monitors::delete(state.pool(), monitor_id).await?;
    state.poke_scheduler();
    Ok(StatusCode::NO_CONTENT)
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<UpdateMonitor>,
) -> Result<Json<Monitor>, ApiError> {
    let monitor_id = parse_monitor_id(&id)?;
    input.validate()?;
    let monitor = rampart_db::monitors::update(state.pool(), monitor_id, input).await?;
    // Interval / url / proxy_id changes need the running probe task to
    // pick up the new config — poke triggers a reload diff.
    state.poke_scheduler();
    Ok(Json(monitor))
}

async fn pause(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let monitor_id = parse_monitor_id(&id)?;
    rampart_db::monitors::set_active(state.pool(), monitor_id, false).await?;
    state.poke_scheduler();
    Ok(StatusCode::NO_CONTENT)
}

async fn resume(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let monitor_id = parse_monitor_id(&id)?;
    rampart_db::monitors::set_active(state.pool(), monitor_id, true).await?;
    state.poke_scheduler();
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
pub struct SummaryQuery {
    /// Rollup window in seconds. Default 24h.
    #[serde(default = "default_window")]
    pub window: i64,
}
fn default_window() -> i64 {
    86_400
}

#[derive(Debug, Serialize)]
pub struct MonitorSummaryDto {
    pub monitor_id: MonitorId,
    pub total: i64,
    pub up: i64,
    pub uptime_pct: Option<f64>,
    pub avg_latency_ms: Option<f64>,
    pub last_status: Option<MonitorStatus>,
    pub last_ts: Option<OffsetDateTime>,
}

async fn summary(
    State(state): State<AppState>,
    Query(q): Query<SummaryQuery>,
) -> Result<Json<Vec<MonitorSummaryDto>>, ApiError> {
    let rows = rampart_db::heartbeats::summary_window(state.pool(), q.window).await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| MonitorSummaryDto {
                monitor_id: r.monitor_id,
                total: r.total,
                up: r.up,
                uptime_pct: if r.total > 0 {
                    Some(r.up as f64 / r.total as f64 * 100.0)
                } else {
                    None
                },
                avg_latency_ms: r.avg_latency_ms,
                last_status: r.last_status,
                last_ts: r.last_ts,
            })
            .collect(),
    ))
}

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    /// How many heartbeats per monitor. Default 60 (the dashboard strip).
    #[serde(default = "default_history_per")]
    pub per: i64,
}
fn default_history_per() -> i64 {
    60
}

async fn history_all(
    State(state): State<AppState>,
    Query(q): Query<HistoryQuery>,
) -> Result<Json<Vec<Heartbeat>>, ApiError> {
    let per = q.per.clamp(1, 500);
    let hbs = rampart_db::heartbeats::recent_per_monitor(state.pool(), per).await?;
    Ok(Json(hbs))
}

#[derive(Debug, Deserialize)]
pub struct HeartbeatsQuery {
    /// Max rows. Default 100. Clamped to 2000.
    #[serde(default = "default_hb_limit")]
    pub limit: i64,
}
fn default_hb_limit() -> i64 {
    100
}

async fn heartbeats(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<HeartbeatsQuery>,
) -> Result<Json<Vec<Heartbeat>>, ApiError> {
    let monitor_id = parse_monitor_id(&id)?;
    let limit = q.limit.clamp(1, 2000);
    let hbs = rampart_db::heartbeats::recent_for_monitor(state.pool(), monitor_id, limit).await?;
    Ok(Json(hbs))
}
