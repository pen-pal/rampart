//! `/v1/notifications` routes.

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use rampart_core::ids::{MonitorId, NotificationId};
use rampart_db::notifications::{
    MonitorChannelCount, NewNotification, Notification, UpdateNotification,
};
use rampart_db::users::User;
use serde::Deserialize;
use std::str::FromStr;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/counts", get(counts))
        .route("/{id}", get(get_one).patch(update).delete(remove))
        .route("/{id}/test", post(send_test))
}

pub fn monitor_attach_router() -> Router<AppState> {
    Router::new()
        .route("/{mid}/notifications", get(list_for_monitor))
        .route("/{mid}/notifications/{nid}", post(attach).delete(detach))
}

fn parse_notif(id: &str) -> Result<NotificationId, ApiError> {
    Uuid::from_str(id)
        .map(NotificationId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid notification id".into()))
}

fn parse_monitor(id: &str) -> Result<MonitorId, ApiError> {
    Uuid::from_str(id)
        .map(MonitorId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid monitor id".into()))
}

async fn list(State(state): State<AppState>) -> Result<Json<Vec<Notification>>, ApiError> {
    Ok(Json(rampart_db::notifications::list(state.pool()).await?))
}

async fn create(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Json(input): Json<NewNotification>,
) -> Result<(StatusCode, Json<Notification>), ApiError> {
    if input.name.trim().is_empty() {
        return Err(ApiError::BadRequest("name is required".into()));
    }
    let n = rampart_db::notifications::create(state.pool(), input).await?;
    crate::audit::record(
        state.pool(),
        &user,
        &headers,
        "notification.create",
        "notification",
        Some(n.id.0),
        Some(serde_json::json!({ "name": n.name, "kind": n.kind })),
    )
    .await;
    Ok((StatusCode::CREATED, Json(n)))
}

async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Notification>, ApiError> {
    let id = parse_notif(&id)?;
    Ok(Json(
        rampart_db::notifications::get(state.pool(), id).await?,
    ))
}

async fn update(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<UpdateNotification>,
) -> Result<Json<Notification>, ApiError> {
    let id = parse_notif(&id)?;
    let n = rampart_db::notifications::update(state.pool(), id, input).await?;
    crate::audit::record(
        state.pool(),
        &user,
        &headers,
        "notification.update",
        "notification",
        Some(id.0),
        Some(serde_json::json!({ "name": n.name, "active": n.active })),
    )
    .await;
    Ok(Json(n))
}

async fn remove(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = parse_notif(&id)?;
    rampart_db::notifications::delete(state.pool(), id).await?;
    crate::audit::record(
        state.pool(),
        &user,
        &headers,
        "notification.delete",
        "notification",
        Some(id.0),
        None,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

async fn counts(State(state): State<AppState>) -> Result<Json<Vec<MonitorChannelCount>>, ApiError> {
    Ok(Json(
        rampart_db::notifications::counts_per_monitor(state.pool()).await?,
    ))
}

#[derive(Deserialize)]
pub struct AttachPath {
    pub mid: String,
    pub nid: String,
}

async fn list_for_monitor(
    State(state): State<AppState>,
    Path(mid): Path<String>,
) -> Result<Json<Vec<Notification>>, ApiError> {
    let mid = parse_monitor(&mid)?;
    Ok(Json(
        rampart_db::notifications::for_monitor(state.pool(), mid).await?,
    ))
}

async fn attach(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path(AttachPath { mid, nid }): Path<AttachPath>,
) -> Result<StatusCode, ApiError> {
    let mid = parse_monitor(&mid)?;
    let nid = parse_notif(&nid)?;
    rampart_db::notifications::attach(state.pool(), mid, nid).await?;
    crate::audit::record(
        state.pool(),
        &user,
        &headers,
        "notification.attach",
        "monitor",
        Some(mid.0),
        Some(serde_json::json!({ "notification_id": nid.0 })),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

async fn detach(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path(AttachPath { mid, nid }): Path<AttachPath>,
) -> Result<StatusCode, ApiError> {
    let mid = parse_monitor(&mid)?;
    let nid = parse_notif(&nid)?;
    rampart_db::notifications::detach(state.pool(), mid, nid).await?;
    crate::audit::record(
        state.pool(),
        &user,
        &headers,
        "notification.detach",
        "monitor",
        Some(mid.0),
        Some(serde_json::json!({ "notification_id": nid.0 })),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /v1/notifications/:id/test` — fire a synthetic notification
/// through the channel so the user can verify their config without
/// waiting for a real status flip. Renders a fixed test payload via the
/// configured channel adapter.
async fn send_test(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = parse_notif(&id)?;
    let n = rampart_db::notifications::get(state.pool(), id).await?;

    // Synthesize a fake monitor + heartbeat for the test payload.
    let now = time::OffsetDateTime::now_utc();
    let test_monitor = rampart_core::Monitor {
        id: rampart_core::ids::MonitorId::new(),
        name: "Rampart test monitor".into(),
        kind: rampart_core::MonitorKind::Http,
        url: Some("https://example.com".into()),
        hostname: None,
        port: None,
        config: serde_json::Value::Null,
        interval_seconds: 60,
        retry_interval_sec: 60,
        max_retries: 0,
        timeout_seconds: 10,
        resend_interval_sec: 0,
        upside_down: false,
        http_method: "GET".into(),
        http_body: None,
        http_headers: None,
        accepted_statuses: vec![200],
        follow_redirect: true,
        ignore_tls: false,
        proxy_id: None,
        agent_id: None,
        push_token: None,
        last_push_at: None,
        active: true,
        current_status: rampart_core::MonitorStatus::Up,
        created_at: now,
        updated_at: now,
        tags: Vec::new(),
        cert_days_left: None,
        cert_subject: None,
        cert_checked_at: None,
        group_id: None,
        slo_target_pct: None,
        slo_window_days: None,
    };
    let test_hb = rampart_core::Heartbeat {
        monitor_id: test_monitor.id,
        ts: now,
        status: rampart_core::MonitorStatus::Up,
        latency_ms: Some(42),
        status_code: Some(200),
        msg: Some("This is a test notification from Rampart.".into()),
        retries: 0,
        important: true,
    };
    let event = rampart_notifier::Event {
        kind: rampart_notifier::EventKind::Test,
        monitor: test_monitor,
        heartbeat: test_hb,
        prev_status: Some(rampart_core::MonitorStatus::Down),
        slo_current_pct: None,
    };
    let subject = rampart_notifier::template::default_subject(&event);
    let body = rampart_notifier::template::default_body(&event);

    rampart_notifier::channels::dispatch(
        n.kind,
        &n.config,
        &subject,
        &body,
        &event,
        state.pool(),
        n.id,
    )
    .await
    .map_err(|e| ApiError::BadRequest(format!("channel dispatch failed: {e}")))?;
    Ok(StatusCode::NO_CONTENT)
}
