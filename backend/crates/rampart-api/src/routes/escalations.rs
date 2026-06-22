//! Escalation policies + episode acknowledge.
//!
//! /v1/escalation-policies          — CRUD (editor; like channels)
//! /v1/monitors/{id}/escalation     — the monitor's open episode, if any
//! /v1/monitors/{id}/escalation/ack — stop the ladder (records who)

use crate::auth::OrgContext;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use rampart_core::escalation::{
    validate_steps, EscalationEpisode, EscalationPolicy, NewEscalationPolicy,
    UpdateEscalationPolicy,
};
use rampart_core::ids::{EscalationPolicyId, MonitorId};
use rampart_db::users::User;
use std::str::FromStr;
use uuid::Uuid;
use validator::Validate;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/episodes", get(open_episodes))
        .route("/episodes/{id}/ack", post(ack_episode_route))
        .route("/{id}", axum::routing::patch(update).delete(delete))
}

/// All currently-open escalation episodes (monitor + rule subjects).
async fn open_episodes(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
) -> Result<Json<Vec<EscalationEpisode>>, ApiError> {
    Ok(Json(
        s.store().list_open_episodes_for_org(org.org_id).await?,
    ))
}

/// Acknowledge any episode by id (stops the ladder). Subject-agnostic, so it
/// works for rule episodes too (monitor episodes also have the per-monitor ack).
async fn ack_episode_route(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<EscalationEpisode>, ApiError> {
    let episode_id =
        Uuid::from_str(&id).map_err(|_| ApiError::BadRequest("invalid episode id".into()))?;
    // Gate through the episode's owning policy's org — cross-org episode = 404.
    s.store().episode_in_org(episode_id, org.org_id).await?;
    let ep = s.store().ack_episode(episode_id, user.id).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "escalation.ack",
        "escalation_episode",
        Some(episode_id),
        Some(serde_json::json!({ "subject_kind": ep.subject_kind, "subject_ref": ep.subject_ref })),
    )
    .await;
    Ok(Json(ep))
}

/// Merged into the /v1/monitors nest.
pub fn monitor_router() -> Router<AppState> {
    Router::new()
        .route("/{id}/escalation", get(episode))
        .route("/{id}/escalation/ack", post(ack))
}

fn parse(s: &str) -> Result<EscalationPolicyId, ApiError> {
    Uuid::from_str(s)
        .map(EscalationPolicyId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid policy id".into()))
}

fn parse_monitor(s: &str) -> Result<MonitorId, ApiError> {
    Uuid::from_str(s)
        .map(MonitorId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid monitor id".into()))
}

async fn list(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
) -> Result<Json<Vec<EscalationPolicy>>, ApiError> {
    Ok(Json(s.store().list_escalation_policies(org.org_id).await?))
}

async fn create(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    Extension(org): Extension<OrgContext>,
    headers: HeaderMap,
    Json(input): Json<NewEscalationPolicy>,
) -> Result<(StatusCode, Json<EscalationPolicy>), ApiError> {
    input
        .validate()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    validate_steps(&input.steps).map_err(ApiError::BadRequest)?;
    let name = input.name.clone();
    let policy = s
        .store()
        .create_escalation_policy(input, org.org_id)
        .await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "escalation_policy.create",
        "escalation_policy",
        Some(policy.id.0),
        Some(serde_json::json!({ "name": name })),
    )
    .await;
    Ok((StatusCode::CREATED, Json(policy)))
}

async fn update(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    Extension(org): Extension<OrgContext>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<UpdateEscalationPolicy>,
) -> Result<Json<EscalationPolicy>, ApiError> {
    let policy_id = parse(&id)?;
    input
        .validate()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    if let Some(steps) = &input.steps {
        validate_steps(steps).map_err(ApiError::BadRequest)?;
    }
    let policy = s
        .store()
        .update_escalation_policy(policy_id, input, org.org_id)
        .await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "escalation_policy.update",
        "escalation_policy",
        Some(policy_id.0),
        None,
    )
    .await;
    Ok(Json(policy))
}

async fn delete(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    Extension(org): Extension<OrgContext>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let policy_id = parse(&id)?;
    s.store()
        .delete_escalation_policy(policy_id, org.org_id)
        .await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "escalation_policy.delete",
        "escalation_policy",
        Some(policy_id.0),
        None,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// The monitor's open episode — `null` body when the ladder is quiet.
async fn episode(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Path(id): Path<String>,
) -> Result<Json<Option<EscalationEpisode>>, ApiError> {
    let monitor_id = parse_monitor(&id)?;
    // Gate through the monitor's org — cross-org monitor id is a 404.
    rampart_db::monitors::get(s.pool(), monitor_id, org.org_id).await?;
    Ok(Json(s.store().open_episode_for_monitor(monitor_id).await?))
}

/// Acknowledge: stops further escalation steps. 404 when nothing is
/// open or it's already acked — the UI treats both as "already handled".
async fn ack(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<EscalationEpisode>, ApiError> {
    let monitor_id = parse_monitor(&id)?;
    // Gate through the monitor's org — cross-org monitor id is a 404.
    rampart_db::monitors::get(s.pool(), monitor_id, org.org_id).await?;
    let episode = s
        .store()
        .ack_episode_for_monitor(monitor_id, user.id)
        .await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "escalation.ack",
        "monitor",
        Some(monitor_id.0),
        None,
    )
    .await;
    Ok(Json(episode))
}
