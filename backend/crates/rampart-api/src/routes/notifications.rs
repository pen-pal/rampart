//! `/v1/notifications` routes.

use crate::auth::OrgContext;
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

/// Sentinel shown in place of a secret config value on the read path. On write,
/// an incoming value still equal to this is restored from the stored config
/// (so editing a channel without re-typing its secrets keeps them).
const REDACTED: &str = "••••••";

/// Channel `config` is decrypted by the DB layer (`secrets::open`) before it
/// reaches here — so without redaction every authenticated user, including
/// read-only ones, could read every channel's plaintext tokens/passwords. We
/// mask secret-shaped keys on all HTTP read responses; the notifier reads
/// config straight from the DB, so delivery is unaffected.
fn is_secret_key(k: &str) -> bool {
    let k = k.to_ascii_lowercase();
    [
        "token",
        "secret",
        "password",
        "passwd",
        "psw",
        "key",
        "dsn",
        "webhook",
        "url",
        "sas",
        "auth",
        "credential",
        "signature",
        "bearer",
    ]
    .iter()
    .any(|p| k.contains(p))
}

fn redact_config(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Object(map) => {
            for (k, val) in map.iter_mut() {
                if is_secret_key(k) && val.is_string() {
                    *val = serde_json::Value::String(REDACTED.to_string());
                } else {
                    redact_config(val);
                }
            }
        }
        serde_json::Value::Array(arr) => arr.iter_mut().for_each(redact_config),
        _ => {}
    }
}

fn redacted(mut n: Notification) -> Notification {
    redact_config(&mut n.config);
    n
}

/// Restore any still-redacted values in an incoming config from the stored one,
/// so an unchanged secret survives an edit instead of being overwritten with
/// the mask sentinel.
fn unredact_config(incoming: &mut serde_json::Value, existing: &serde_json::Value) {
    if let (Some(io), Some(eo)) = (incoming.as_object_mut(), existing.as_object()) {
        for (k, iv) in io.iter_mut() {
            if iv.as_str() == Some(REDACTED) {
                if let Some(ev) = eo.get(k) {
                    *iv = ev.clone();
                }
            } else if let Some(ev) = eo.get(k) {
                unredact_config(iv, ev);
            }
        }
    }
}

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

async fn list(
    State(state): State<AppState>,
    Extension(org): Extension<OrgContext>,
) -> Result<Json<Vec<Notification>>, ApiError> {
    let chans = state.store().list_notifications(org.org_id).await?;
    Ok(Json(chans.into_iter().map(redacted).collect()))
}

async fn create(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Extension(org): Extension<OrgContext>,
    headers: HeaderMap,
    Json(input): Json<NewNotification>,
) -> Result<(StatusCode, Json<Notification>), ApiError> {
    if input.name.trim().is_empty() {
        return Err(ApiError::BadRequest("name is required".into()));
    }
    let n = state.store().create_notification(input, org.org_id).await?;
    crate::audit::record(
        state.store(),
        &user,
        &headers,
        "notification.create",
        "notification",
        Some(n.id.0),
        Some(serde_json::json!({ "name": n.name, "kind": n.kind })),
    )
    .await;
    Ok((StatusCode::CREATED, Json(redacted(n))))
}

async fn get_one(
    State(state): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Path(id): Path<String>,
) -> Result<Json<Notification>, ApiError> {
    let id = parse_notif(&id)?;
    Ok(Json(redacted(
        state.store().get_notification(id, org.org_id).await?,
    )))
}

async fn update(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Extension(org): Extension<OrgContext>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(mut input): Json<UpdateNotification>,
) -> Result<Json<Notification>, ApiError> {
    let id = parse_notif(&id)?;
    // If the client sent back redacted secrets unchanged, restore them from the
    // stored config so an edit doesn't clobber tokens the user never saw.
    if let Some(cfg) = input.config.as_mut() {
        if let Ok(existing) = state.store().get_notification(id, org.org_id).await {
            unredact_config(cfg, &existing.config);
        }
    }
    let n = state
        .store()
        .update_notification(id, input, org.org_id)
        .await?;
    crate::audit::record(
        state.store(),
        &user,
        &headers,
        "notification.update",
        "notification",
        Some(id.0),
        Some(serde_json::json!({ "name": n.name, "active": n.active })),
    )
    .await;
    Ok(Json(redacted(n)))
}

async fn remove(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Extension(org): Extension<OrgContext>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = parse_notif(&id)?;
    state.store().delete_notification(id, org.org_id).await?;
    crate::audit::record(
        state.store(),
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

async fn counts(
    State(state): State<AppState>,
    Extension(org): Extension<OrgContext>,
) -> Result<Json<Vec<MonitorChannelCount>>, ApiError> {
    Ok(Json(
        state
            .store()
            .notification_counts_per_monitor(org.org_id)
            .await?,
    ))
}

#[derive(Deserialize)]
pub struct AttachPath {
    pub mid: String,
    pub nid: String,
}

async fn list_for_monitor(
    State(state): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Path(mid): Path<String>,
) -> Result<Json<Vec<Notification>>, ApiError> {
    let mid = parse_monitor(&mid)?;
    // Gate through the owning monitor's org — a cross-org monitor id is a 404,
    // so its attached channels can't be enumerated.
    state.store().get_monitor(mid, org.org_id).await?;
    let chans = state.store().notifications_for_monitor(mid).await?;
    Ok(Json(chans.into_iter().map(redacted).collect()))
}

async fn attach(
    State(state): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path(AttachPath { mid, nid }): Path<AttachPath>,
) -> Result<StatusCode, ApiError> {
    let mid = parse_monitor(&mid)?;
    let nid = parse_notif(&nid)?;
    // Both the monitor and the channel must belong to the caller's org.
    state.store().get_monitor(mid, org.org_id).await?;
    state.store().get_notification(nid, org.org_id).await?;
    state.store().attach_notification(mid, nid).await?;
    crate::audit::record(
        state.store(),
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
    Extension(org): Extension<OrgContext>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path(AttachPath { mid, nid }): Path<AttachPath>,
) -> Result<StatusCode, ApiError> {
    let mid = parse_monitor(&mid)?;
    let nid = parse_notif(&nid)?;
    // Both the monitor and the channel must belong to the caller's org.
    state.store().get_monitor(mid, org.org_id).await?;
    state.store().get_notification(nid, org.org_id).await?;
    state.store().detach_notification(mid, nid).await?;
    crate::audit::record(
        state.store(),
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
    Extension(org): Extension<OrgContext>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = parse_notif(&id)?;
    let n = state.store().get_notification(id, org.org_id).await?;

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
        escalation_policy_id: None,
        push_token: None,
        last_push_at: None,
        last_run_started_at: None,
        active: true,
        current_status: rampart_core::MonitorStatus::Up,
        created_at: now,
        updated_at: now,
        tags: Vec::new(),
        cert_days_left: None,
        cert_subject: None,
        cert_checked_at: None,
        check_cert: false,
        cert_expiry_days: 14,
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

#[cfg(test)]
mod redaction_tests {
    use super::{redact_config, unredact_config, REDACTED};
    use serde_json::json;

    #[test]
    fn redacts_secret_keys_keeps_plain_ones() {
        let mut c = json!({
            "webhook_url": "https://hooks/secret-token",
            "bot_token": "abc123",
            "api_key": "k-9",
            "channel": "#ops",
            "port": 443,
            "from": "alerts@example.com"
        });
        redact_config(&mut c);
        assert_eq!(c["webhook_url"], REDACTED);
        assert_eq!(c["bot_token"], REDACTED);
        assert_eq!(c["api_key"], REDACTED);
        assert_eq!(c["channel"], "#ops"); // non-secret stays visible
        assert_eq!(c["port"], 443); // non-string untouched
        assert_eq!(c["from"], "alerts@example.com");
    }

    #[test]
    fn unredact_restores_unchanged_but_keeps_edits() {
        let existing = json!({ "bot_token": "real-token", "channel": "#ops" });
        let mut incoming = json!({ "bot_token": REDACTED, "channel": "#new" });
        unredact_config(&mut incoming, &existing);
        assert_eq!(incoming["bot_token"], "real-token"); // masked → restored
        assert_eq!(incoming["channel"], "#new"); // edited → kept
    }

    #[test]
    fn unredact_keeps_a_freshly_typed_secret() {
        let existing = json!({ "bot_token": "old" });
        let mut incoming = json!({ "bot_token": "new-token" });
        unredact_config(&mut incoming, &existing);
        assert_eq!(incoming["bot_token"], "new-token");
    }
}
