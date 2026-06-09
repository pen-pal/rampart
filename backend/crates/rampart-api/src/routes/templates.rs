//! `/v1/notification-templates` routes.

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use rampart_core::ids::NotificationTemplateId;
use rampart_db::templates::{NewTemplate, Template, UpdateTemplate};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/preview", post(preview))
        .route("/{id}", get(get_one).patch(update).delete(remove))
}

fn parse(id: &str) -> Result<NotificationTemplateId, ApiError> {
    Uuid::from_str(id)
        .map(NotificationTemplateId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid template id".into()))
}

async fn list(State(s): State<AppState>) -> Result<Json<Vec<Template>>, ApiError> {
    Ok(Json(rampart_db::templates::list(s.pool()).await?))
}

async fn get_one(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Template>, ApiError> {
    Ok(Json(
        rampart_db::templates::get(s.pool(), parse(&id)?).await?,
    ))
}

async fn create(
    State(s): State<AppState>,
    Json(input): Json<NewTemplate>,
) -> Result<(StatusCode, Json<Template>), ApiError> {
    if input.name.trim().is_empty() {
        return Err(ApiError::BadRequest("name is required".into()));
    }
    if input.body_template.trim().is_empty() {
        return Err(ApiError::BadRequest("body_template is required".into()));
    }
    let t = rampart_db::templates::create(s.pool(), input).await?;
    Ok((StatusCode::CREATED, Json(t)))
}

async fn update(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<UpdateTemplate>,
) -> Result<Json<Template>, ApiError> {
    Ok(Json(
        rampart_db::templates::update(s.pool(), parse(&id)?, input).await?,
    ))
}

async fn remove(State(s): State<AppState>, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    rampart_db::templates::delete(s.pool(), parse(&id)?).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct PreviewBody {
    #[serde(default)]
    subject_template: String,
    #[serde(default)]
    body_template: String,
}

#[derive(Serialize)]
struct PreviewResponse {
    subject: String,
    body: String,
}

/// Render the subject + body templates against a canned fake heartbeat
/// so the user can see what the notification will actually look like
/// without firing a real channel send. No DB write, no state change —
/// pure pass-through to the Liquid renderer the notifier uses.
async fn preview(Json(input): Json<PreviewBody>) -> Result<Json<PreviewResponse>, ApiError> {
    use rampart_core::{ids::MonitorId, Heartbeat, Monitor, MonitorKind, MonitorStatus};
    use rampart_notifier::{template::render, Event, EventKind};
    use time::OffsetDateTime;

    let now = OffsetDateTime::now_utc();
    let monitor = Monitor {
        id: MonitorId::new(),
        name: "example-monitor".into(),
        kind: MonitorKind::Http,
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
        push_token: None,
        last_push_at: None,
        active: true,
        current_status: MonitorStatus::Down,
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
    let hb = Heartbeat {
        monitor_id: monitor.id,
        ts: now,
        status: MonitorStatus::Down,
        latency_ms: Some(417),
        status_code: Some(503),
        msg: Some("Service Unavailable".into()),
        retries: 0,
        important: true,
    };
    let event = Event {
        kind: EventKind::Test,
        monitor,
        heartbeat: hb,
        prev_status: Some(MonitorStatus::Up),
    };
    Ok(Json(PreviewResponse {
        subject: render(&input.subject_template, &event),
        body: render(&input.body_template, &event),
    }))
}
