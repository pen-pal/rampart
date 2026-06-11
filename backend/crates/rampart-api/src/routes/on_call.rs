//! On-call schedules — channel rotations referenced by escalation steps.
//!
//! /v1/on-call-schedules            — CRUD (editor; like channels)
//! /v1/on-call-schedules/{id}/current — the channel on call right now

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use axum::{Json, Router};
use rampart_core::ids::{NotificationId, OnCallScheduleId};
use rampart_core::on_call::{
    validate_rotation, NewOnCallSchedule, OnCallSchedule, UpdateOnCallSchedule,
};
use rampart_db::users::User;
use serde::Serialize;
use std::str::FromStr;
use time::OffsetDateTime;
use uuid::Uuid;
use validator::Validate;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", axum::routing::patch(update).delete(delete))
        .route("/{id}/current", get(current))
}

fn parse(s: &str) -> Result<OnCallScheduleId, ApiError> {
    Uuid::from_str(s)
        .map(OnCallScheduleId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid schedule id".into()))
}

async fn list(State(s): State<AppState>) -> Result<Json<Vec<OnCallSchedule>>, ApiError> {
    Ok(Json(rampart_db::on_call::list(s.pool()).await?))
}

async fn create(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Json(input): Json<NewOnCallSchedule>,
) -> Result<(StatusCode, Json<OnCallSchedule>), ApiError> {
    input
        .validate()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    validate_rotation(input.rotation_seconds, &input.participant_ids)
        .map_err(ApiError::BadRequest)?;
    let name = input.name.clone();
    let schedule = rampart_db::on_call::create(s.pool(), input).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "on_call_schedule.create",
        "on_call_schedule",
        Some(schedule.id.0),
        Some(serde_json::json!({ "name": name })),
    )
    .await;
    Ok((StatusCode::CREATED, Json(schedule)))
}

async fn update(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<UpdateOnCallSchedule>,
) -> Result<Json<OnCallSchedule>, ApiError> {
    let schedule_id = parse(&id)?;
    input
        .validate()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    // Validate the rotation only against the fields actually being changed,
    // falling back to the stored values for whatever is omitted.
    if input.rotation_seconds.is_some() || input.participant_ids.is_some() {
        let current = rampart_db::on_call::get(s.pool(), schedule_id).await?;
        let rotation = input.rotation_seconds.unwrap_or(current.rotation_seconds);
        let participants = input
            .participant_ids
            .clone()
            .unwrap_or(current.participant_ids);
        validate_rotation(rotation, &participants).map_err(ApiError::BadRequest)?;
    }
    let schedule = rampart_db::on_call::update(s.pool(), schedule_id, input).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "on_call_schedule.update",
        "on_call_schedule",
        Some(schedule_id.0),
        None,
    )
    .await;
    Ok(Json(schedule))
}

async fn delete(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let schedule_id = parse(&id)?;
    rampart_db::on_call::delete(s.pool(), schedule_id).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "on_call_schedule.delete",
        "on_call_schedule",
        Some(schedule_id.0),
        None,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
struct CurrentOnCall {
    /// The channel on call right now, or `null` for an empty/malformed ring.
    on_call: Option<NotificationId>,
}

/// Who's on call for this schedule at the current instant. 404 if the
/// schedule doesn't exist.
async fn current(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<CurrentOnCall>, ApiError> {
    let schedule_id = parse(&id)?;
    let on_call =
        rampart_db::on_call::current_channel(s.pool(), schedule_id, OffsetDateTime::now_utc())
            .await?;
    Ok(Json(CurrentOnCall { on_call }))
}
