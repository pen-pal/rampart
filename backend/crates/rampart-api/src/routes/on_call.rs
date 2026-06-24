//! On-call schedules — channel rotations referenced by escalation steps.
//!
//! /v1/on-call-schedules            — CRUD (editor; like channels)
//! /v1/on-call-schedules/{id}/current — the channel on call right now

use crate::auth::OrgContext;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use axum::{Json, Router};
use rampart_core::ids::{NotificationId, OnCallScheduleId, UserId};
use rampart_core::on_call::{NewOnCallSchedule, OnCallSchedule, UpdateOnCallSchedule};
use rampart_db::users::User;
use rampart_db::DbError;
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

/// Reject participant channels/users owned by ANOTHER org. The ring is resolved
/// unscoped at fire time, so without this gate an editor could page (or pull the
/// email of) another tenant's channel/user. Mirrors the routing.rs double-gate;
/// a body ref to a foreign/absent id is a 400, not a 404.
async fn gate_participant_refs(
    s: &AppState,
    channels: &[NotificationId],
    users: &[UserId],
    org: &OrgContext,
) -> Result<(), ApiError> {
    for c in channels {
        match s.store().get_notification(*c, org.org_id).await {
            Ok(_) => {}
            Err(DbError::NotFound) => {
                return Err(ApiError::BadRequest(format!(
                    "schedule references channel {c} not in this org"
                )))
            }
            Err(e) => return Err(ApiError::Db(e)),
        }
    }
    for u in users {
        if s.store().org_member_role(org.org_id, *u).await?.is_none() {
            return Err(ApiError::BadRequest(format!(
                "schedule references user {u} who is not a member of this org"
            )));
        }
    }
    Ok(())
}

async fn list(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
) -> Result<Json<Vec<OnCallSchedule>>, ApiError> {
    Ok(Json(s.store().list_on_call(org.org_id).await?))
}

async fn create(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    Extension(org): Extension<OrgContext>,
    headers: HeaderMap,
    Json(input): Json<NewOnCallSchedule>,
) -> Result<(StatusCode, Json<OnCallSchedule>), ApiError> {
    input
        .validate()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    rampart_core::on_call::validate_schedule(
        input.rotation_seconds,
        input.participant_ids.len() + input.participant_user_ids.len(),
    )
    .map_err(ApiError::BadRequest)?;
    gate_participant_refs(
        &s,
        &input.participant_ids,
        &input.participant_user_ids,
        &org,
    )
    .await?;
    let name = input.name.clone();
    let schedule = s.store().create_on_call(input, org.org_id).await?;
    crate::audit::record(
        s.store(),
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
    Extension(org): Extension<OrgContext>,
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
    if input.rotation_seconds.is_some()
        || input.participant_ids.is_some()
        || input.participant_user_ids.is_some()
    {
        let current = s.store().get_on_call(schedule_id, org.org_id).await?;
        let rotation = input.rotation_seconds.unwrap_or(current.rotation_seconds);
        let chans = input
            .participant_ids
            .clone()
            .unwrap_or(current.participant_ids)
            .len();
        let users = input
            .participant_user_ids
            .clone()
            .unwrap_or(current.participant_user_ids)
            .len();
        rampart_core::on_call::validate_schedule(rotation, chans + users)
            .map_err(ApiError::BadRequest)?;
    }
    // Gate only the refs actually being written this request; omitted lists keep
    // their stored (already-gated) values.
    if let Some(ch) = &input.participant_ids {
        gate_participant_refs(&s, ch, &[], &org).await?;
    }
    if let Some(us) = &input.participant_user_ids {
        gate_participant_refs(&s, &[], us, &org).await?;
    }
    let schedule = s
        .store()
        .update_on_call(schedule_id, input, org.org_id)
        .await?;
    crate::audit::record(
        s.store(),
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
    Extension(org): Extension<OrgContext>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let schedule_id = parse(&id)?;
    s.store().delete_on_call(schedule_id, org.org_id).await?;
    crate::audit::record(
        s.store(),
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
    /// Who's on call right now — a `{kind:"channel"|"user", id}` target, or
    /// `null` for an empty/malformed ring. Previously resolved only channel
    /// rings and returned null when a *user* was on call.
    on_call: Option<rampart_core::on_call::OnCallTarget>,
}

/// Who's on call for this schedule at the current instant. 404 if the
/// schedule doesn't exist.
async fn current(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Path(id): Path<String>,
) -> Result<Json<CurrentOnCall>, ApiError> {
    let schedule_id = parse(&id)?;
    // Gate through the schedule's org — a cross-org schedule id is a 404.
    // `current_target` stays unscoped (the notifier resolves it at alert
    // time), so the org check lives here.
    s.store().get_on_call(schedule_id, org.org_id).await?;
    let on_call = s
        .store()
        .oncall_current_target(schedule_id, OffsetDateTime::now_utc())
        .await?;
    Ok(Json(CurrentOnCall { on_call }))
}
