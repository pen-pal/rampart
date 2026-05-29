//! Tag-routing API: tags on folders + channels, folder-level channel
//! attach, per-monitor channel exclude, and a resolved-channels read.
//!
//! Routers merge into existing nests so the URLs read naturally:
//!   * group_router    → /v1/monitor-groups/:id/{tags,channels}/...
//!   * channel_router  → /v1/notifications/:id/tags/...
//!   * monitor_router  → /v1/monitors/:id/{excludes,effective-channels}/...

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use rampart_core::ids::{MonitorGroupId, MonitorId, NotificationId, TagId};
use std::str::FromStr;
use uuid::Uuid;

pub fn group_router() -> Router<AppState> {
    Router::new()
        .route("/:id/tags", get(list_group_tags))
        .route("/:id/tags/:tag_id", post(add_group_tag).delete(del_group_tag))
        .route("/:id/channels", get(list_group_channels))
        .route("/:id/channels/:notif_id", post(add_group_channel).delete(del_group_channel))
}

pub fn channel_router() -> Router<AppState> {
    Router::new()
        .route("/:id/tags", get(list_channel_tags))
        .route("/:id/tags/:tag_id", post(add_channel_tag).delete(del_channel_tag))
}

pub fn monitor_router() -> Router<AppState> {
    Router::new()
        .route("/:id/effective-channels", get(effective_channels))
        .route("/:id/excludes", get(list_excludes))
        .route("/:id/excludes/:notif_id", post(add_exclude).delete(del_exclude))
}

fn pg(s: &str) -> Result<MonitorGroupId, ApiError> {
    Uuid::from_str(s).map(MonitorGroupId::from_uuid).map_err(|_| ApiError::BadRequest("invalid group id".into()))
}
fn pm(s: &str) -> Result<MonitorId, ApiError> {
    Uuid::from_str(s).map(MonitorId::from_uuid).map_err(|_| ApiError::BadRequest("invalid monitor id".into()))
}
fn pn(s: &str) -> Result<NotificationId, ApiError> {
    Uuid::from_str(s).map(NotificationId::from_uuid).map_err(|_| ApiError::BadRequest("invalid notification id".into()))
}
fn pt(s: &str) -> Result<TagId, ApiError> {
    Uuid::from_str(s).map(TagId::from_uuid).map_err(|_| ApiError::BadRequest("invalid tag id".into()))
}

// ── folder tags ───────────────────────────────────────────────────────────
async fn list_group_tags(State(s): State<AppState>, Path(id): Path<String>) -> Result<Json<Vec<Uuid>>, ApiError> {
    let ids = rampart_db::routing::group_tag_ids(s.pool(), pg(&id)?).await?;
    Ok(Json(ids.into_iter().map(|t| t.0).collect()))
}
async fn add_group_tag(State(s): State<AppState>, Path((id, tag)): Path<(String, String)>) -> Result<StatusCode, ApiError> {
    rampart_db::routing::tag_group(s.pool(), pg(&id)?, pt(&tag)?).await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn del_group_tag(State(s): State<AppState>, Path((id, tag)): Path<(String, String)>) -> Result<StatusCode, ApiError> {
    rampart_db::routing::untag_group(s.pool(), pg(&id)?, pt(&tag)?).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── folder channels ─────────────────────────────────────────────────────────
async fn list_group_channels(State(s): State<AppState>, Path(id): Path<String>) -> Result<Json<Vec<Uuid>>, ApiError> {
    let ids = rampart_db::routing::group_channel_ids(s.pool(), pg(&id)?).await?;
    Ok(Json(ids.into_iter().map(|n| n.0).collect()))
}
async fn add_group_channel(State(s): State<AppState>, Path((id, notif)): Path<(String, String)>) -> Result<StatusCode, ApiError> {
    rampart_db::routing::attach_group_channel(s.pool(), pg(&id)?, pn(&notif)?).await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn del_group_channel(State(s): State<AppState>, Path((id, notif)): Path<(String, String)>) -> Result<StatusCode, ApiError> {
    rampart_db::routing::detach_group_channel(s.pool(), pg(&id)?, pn(&notif)?).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── channel tags ────────────────────────────────────────────────────────────
async fn list_channel_tags(State(s): State<AppState>, Path(id): Path<String>) -> Result<Json<Vec<Uuid>>, ApiError> {
    let ids = rampart_db::routing::channel_tag_ids(s.pool(), pn(&id)?).await?;
    Ok(Json(ids.into_iter().map(|t| t.0).collect()))
}
async fn add_channel_tag(State(s): State<AppState>, Path((id, tag)): Path<(String, String)>) -> Result<StatusCode, ApiError> {
    rampart_db::routing::tag_channel(s.pool(), pn(&id)?, pt(&tag)?).await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn del_channel_tag(State(s): State<AppState>, Path((id, tag)): Path<(String, String)>) -> Result<StatusCode, ApiError> {
    rampart_db::routing::untag_channel(s.pool(), pn(&id)?, pt(&tag)?).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── monitor excludes + resolved view ─────────────────────────────────────────
async fn list_excludes(State(s): State<AppState>, Path(id): Path<String>) -> Result<Json<Vec<Uuid>>, ApiError> {
    let ids = rampart_db::routing::monitor_exclude_ids(s.pool(), pm(&id)?).await?;
    Ok(Json(ids.into_iter().map(|n| n.0).collect()))
}
async fn add_exclude(State(s): State<AppState>, Path((id, notif)): Path<(String, String)>) -> Result<StatusCode, ApiError> {
    rampart_db::routing::exclude_channel(s.pool(), pm(&id)?, pn(&notif)?).await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn del_exclude(State(s): State<AppState>, Path((id, notif)): Path<(String, String)>) -> Result<StatusCode, ApiError> {
    rampart_db::routing::unexclude_channel(s.pool(), pm(&id)?, pn(&notif)?).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The channels that WILL fire for this monitor, after tag/folder/exclude
/// resolution — so the UI can show the effective set, not just explicit
/// attachments. Returns notification ids.
async fn effective_channels(State(s): State<AppState>, Path(id): Path<String>) -> Result<Json<Vec<Uuid>>, ApiError> {
    let chans = rampart_db::routing::resolve_channels_for_monitor(s.pool(), pm(&id)?).await?;
    Ok(Json(chans.into_iter().map(|c| c.id.0).collect()))
}
