//! Tag-routing API: tags on folders + channels, folder-level channel
//! attach, per-monitor channel exclude, and a resolved-channels read.
//!
//! Routers merge into existing nests so the URLs read naturally:
//!   * group_router    → /v1/monitor-groups/:id/{tags,channels}/...
//!   * channel_router  → /v1/notifications/:id/tags/...
//!   * monitor_router  → /v1/monitors/:id/{excludes,effective-channels}/...
//!
//! Every handler is org-gated through the root entity it touches (folder,
//! channel, monitor) plus, where a second entity is named (tag / channel),
//! that entity too — so a cross-org id on either end is a 404 and routing
//! relationships never cross tenants. The notifier's own resolution path
//! (`routing::resolve_channels_for_monitor`) stays unscoped — it must see the
//! full picture at alert time.

use crate::auth::OrgContext;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use rampart_core::ids::{MonitorGroupId, MonitorId, NotificationId, TagId};
use rampart_db::users::User;
use std::str::FromStr;
use uuid::Uuid;

pub fn group_router() -> Router<AppState> {
    Router::new()
        .route("/{id}/tags", get(list_group_tags))
        .route(
            "/{id}/tags/{tag_id}",
            post(add_group_tag).delete(del_group_tag),
        )
        .route("/{id}/channels", get(list_group_channels))
        .route(
            "/{id}/channels/{notif_id}",
            post(add_group_channel).delete(del_group_channel),
        )
}

pub fn channel_router() -> Router<AppState> {
    Router::new()
        .route("/{id}/tags", get(list_channel_tags))
        .route(
            "/{id}/tags/{tag_id}",
            post(add_channel_tag).delete(del_channel_tag),
        )
}

pub fn monitor_router() -> Router<AppState> {
    Router::new()
        .route("/{id}/effective-channels", get(effective_channels))
        .route("/{id}/excludes", get(list_excludes))
        .route(
            "/{id}/excludes/{notif_id}",
            post(add_exclude).delete(del_exclude),
        )
}

fn pg(s: &str) -> Result<MonitorGroupId, ApiError> {
    Uuid::from_str(s)
        .map(MonitorGroupId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid group id".into()))
}
fn pm(s: &str) -> Result<MonitorId, ApiError> {
    Uuid::from_str(s)
        .map(MonitorId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid monitor id".into()))
}
fn pn(s: &str) -> Result<NotificationId, ApiError> {
    Uuid::from_str(s)
        .map(NotificationId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid notification id".into()))
}
fn pt(s: &str) -> Result<TagId, ApiError> {
    Uuid::from_str(s)
        .map(TagId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid tag id".into()))
}

// ── org gates ───────────────────────────────────────────────────────────────
// Each returns 404 (NotFound) when the entity lives in another org.
async fn gate_group(s: &AppState, g: MonitorGroupId, org: &OrgContext) -> Result<(), ApiError> {
    rampart_db::monitor_groups::in_org(s.pool(), g, org.org_id).await?;
    Ok(())
}
async fn gate_channel(s: &AppState, n: NotificationId, org: &OrgContext) -> Result<(), ApiError> {
    rampart_db::notifications::get(s.pool(), n, org.org_id).await?;
    Ok(())
}
async fn gate_tag(s: &AppState, t: TagId, org: &OrgContext) -> Result<(), ApiError> {
    rampart_db::tags::get(s.pool(), t, org.org_id).await?;
    Ok(())
}
async fn gate_monitor(s: &AppState, m: MonitorId, org: &OrgContext) -> Result<(), ApiError> {
    rampart_db::monitors::get(s.pool(), m, org.org_id).await?;
    Ok(())
}

// ── folder tags ───────────────────────────────────────────────────────────
async fn list_group_tags(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Path(id): Path<String>,
) -> Result<Json<Vec<Uuid>>, ApiError> {
    let gid = pg(&id)?;
    gate_group(&s, gid, &org).await?;
    let ids = rampart_db::routing::group_tag_ids(s.pool(), gid).await?;
    Ok(Json(ids.into_iter().map(|t| t.0).collect()))
}
async fn add_group_tag(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path((id, tag)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let gid = pg(&id)?;
    let tid = pt(&tag)?;
    gate_group(&s, gid, &org).await?;
    gate_tag(&s, tid, &org).await?;
    rampart_db::routing::tag_group(s.pool(), gid, tid).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "routing.tag_group",
        "monitor_group",
        Some(gid.0),
        Some(serde_json::json!({ "tag_id": tid.0 })),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}
async fn del_group_tag(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path((id, tag)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let gid = pg(&id)?;
    let tid = pt(&tag)?;
    gate_group(&s, gid, &org).await?;
    gate_tag(&s, tid, &org).await?;
    rampart_db::routing::untag_group(s.pool(), gid, tid).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "routing.untag_group",
        "monitor_group",
        Some(gid.0),
        Some(serde_json::json!({ "tag_id": tid.0 })),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

// ── folder channels ─────────────────────────────────────────────────────────
async fn list_group_channels(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Path(id): Path<String>,
) -> Result<Json<Vec<Uuid>>, ApiError> {
    let gid = pg(&id)?;
    gate_group(&s, gid, &org).await?;
    let ids = rampart_db::routing::group_channel_ids(s.pool(), gid).await?;
    Ok(Json(ids.into_iter().map(|n| n.0).collect()))
}
async fn add_group_channel(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path((id, notif)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let gid = pg(&id)?;
    let nid = pn(&notif)?;
    gate_group(&s, gid, &org).await?;
    gate_channel(&s, nid, &org).await?;
    rampart_db::routing::attach_group_channel(s.pool(), gid, nid).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "routing.attach_group_channel",
        "monitor_group",
        Some(gid.0),
        Some(serde_json::json!({ "notification_id": nid.0 })),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}
async fn del_group_channel(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path((id, notif)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let gid = pg(&id)?;
    let nid = pn(&notif)?;
    gate_group(&s, gid, &org).await?;
    gate_channel(&s, nid, &org).await?;
    rampart_db::routing::detach_group_channel(s.pool(), gid, nid).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "routing.detach_group_channel",
        "monitor_group",
        Some(gid.0),
        Some(serde_json::json!({ "notification_id": nid.0 })),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

// ── channel tags ────────────────────────────────────────────────────────────
async fn list_channel_tags(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Path(id): Path<String>,
) -> Result<Json<Vec<Uuid>>, ApiError> {
    let nid = pn(&id)?;
    gate_channel(&s, nid, &org).await?;
    let ids = rampart_db::routing::channel_tag_ids(s.pool(), nid).await?;
    Ok(Json(ids.into_iter().map(|t| t.0).collect()))
}
async fn add_channel_tag(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path((id, tag)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let nid = pn(&id)?;
    let tid = pt(&tag)?;
    gate_channel(&s, nid, &org).await?;
    gate_tag(&s, tid, &org).await?;
    rampart_db::routing::tag_channel(s.pool(), nid, tid).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "routing.tag_channel",
        "notification",
        Some(nid.0),
        Some(serde_json::json!({ "tag_id": tid.0 })),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}
async fn del_channel_tag(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path((id, tag)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let nid = pn(&id)?;
    let tid = pt(&tag)?;
    gate_channel(&s, nid, &org).await?;
    gate_tag(&s, tid, &org).await?;
    rampart_db::routing::untag_channel(s.pool(), nid, tid).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "routing.untag_channel",
        "notification",
        Some(nid.0),
        Some(serde_json::json!({ "tag_id": tid.0 })),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

// ── monitor excludes + resolved view ─────────────────────────────────────────
async fn list_excludes(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Path(id): Path<String>,
) -> Result<Json<Vec<Uuid>>, ApiError> {
    let mid = pm(&id)?;
    gate_monitor(&s, mid, &org).await?;
    let ids = rampart_db::routing::monitor_exclude_ids(s.pool(), mid).await?;
    Ok(Json(ids.into_iter().map(|n| n.0).collect()))
}
async fn add_exclude(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path((id, notif)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let mid = pm(&id)?;
    let nid = pn(&notif)?;
    gate_monitor(&s, mid, &org).await?;
    gate_channel(&s, nid, &org).await?;
    rampart_db::routing::exclude_channel(s.pool(), mid, nid).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "routing.exclude_channel",
        "monitor",
        Some(mid.0),
        Some(serde_json::json!({ "notification_id": nid.0 })),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}
async fn del_exclude(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path((id, notif)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let mid = pm(&id)?;
    let nid = pn(&notif)?;
    gate_monitor(&s, mid, &org).await?;
    gate_channel(&s, nid, &org).await?;
    rampart_db::routing::unexclude_channel(s.pool(), mid, nid).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "routing.unexclude_channel",
        "monitor",
        Some(mid.0),
        Some(serde_json::json!({ "notification_id": nid.0 })),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// The channels that WILL fire for this monitor, after tag/folder/exclude
/// resolution — so the UI can show the effective set, not just explicit
/// attachments. Returns notification ids.
async fn effective_channels(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Path(id): Path<String>,
) -> Result<Json<Vec<Uuid>>, ApiError> {
    let mid = pm(&id)?;
    gate_monitor(&s, mid, &org).await?;
    let chans = rampart_db::routing::resolve_channels_for_monitor(s.pool(), mid).await?;
    Ok(Json(chans.into_iter().map(|c| c.id.0).collect()))
}
