//! Web Push subscription endpoints.
//!
//! - `GET  /v1/webpush/vapid-key` — the shared VAPID public key the
//!   browser needs to call `pushManager.subscribe`. Generated + persisted
//!   on first request.
//! - `POST /v1/webpush/subscriptions` — register a browser subscription
//!   against a `webpush` notification channel.
//! - `DELETE /v1/webpush/subscriptions` — unsubscribe by endpoint.

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use rampart_core::ids::NotificationId;
use rampart_notifier::channels::webpush_crypto::generate_vapid_keys;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/vapid-key", get(vapid_key))
        .route(
            "/subscriptions",
            post(subscribe).delete(unsubscribe),
        )
}

#[derive(Serialize)]
struct VapidKeyResp {
    public_key: String,
}

async fn vapid_key(State(s): State<AppState>) -> Result<Json<VapidKeyResp>, ApiError> {
    let keys = rampart_db::webpush::get_or_create_vapid(s.pool(), generate_vapid_keys).await?;
    Ok(Json(VapidKeyResp { public_key: keys.public }))
}

#[derive(Deserialize)]
struct SubscribeInput {
    notification_id: String,
    endpoint: String,
    keys: SubscribeKeys,
}
#[derive(Deserialize)]
struct SubscribeKeys {
    p256dh: String,
    auth:   String,
}

fn parse_notif(s: &str) -> Result<NotificationId, ApiError> {
    Uuid::from_str(s)
        .map(NotificationId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid notification id".into()))
}

async fn subscribe(
    State(s): State<AppState>,
    Json(input): Json<SubscribeInput>,
) -> Result<StatusCode, ApiError> {
    if input.endpoint.is_empty() || input.keys.p256dh.is_empty() || input.keys.auth.is_empty() {
        return Err(ApiError::BadRequest("endpoint + keys required".into()));
    }
    let nid = parse_notif(&input.notification_id)?;
    rampart_db::webpush::upsert(
        s.pool(),
        nid,
        &input.endpoint,
        &input.keys.p256dh,
        &input.keys.auth,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct UnsubscribeInput {
    endpoint: String,
}

async fn unsubscribe(
    State(s): State<AppState>,
    Json(input): Json<UnsubscribeInput>,
) -> Result<StatusCode, ApiError> {
    rampart_db::webpush::delete_by_endpoint(s.pool(), &input.endpoint).await?;
    Ok(StatusCode::NO_CONTENT)
}
