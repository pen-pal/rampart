//! Web Push subscription endpoints.
//!
//! - `GET  /v1/webpush/vapid-key` — the shared VAPID public key the
//!   browser needs to call `pushManager.subscribe`. Generated + persisted
//!   on first request.
//! - `POST /v1/webpush/subscriptions` — register a browser subscription
//!   against a `webpush` notification channel.
//! - `DELETE /v1/webpush/subscriptions` — unsubscribe by endpoint.

use crate::auth::OrgContext;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Extension, State};
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
        .route("/subscriptions", post(subscribe).delete(unsubscribe))
}

#[derive(Serialize)]
struct VapidKeyResp {
    public_key: String,
}

async fn vapid_key(State(s): State<AppState>) -> Result<Json<VapidKeyResp>, ApiError> {
    // Get-or-create composed from the two object-safe store primitives so the
    // call routes through the seam (the generic key generator can't live on a
    // `dyn Store` method). First-call generation persists the new keypair.
    let keys = match s.store().get_vapid_keys().await? {
        Some(keys) => keys,
        None => {
            let (public, private) = generate_vapid_keys();
            let keys = rampart_db::webpush::VapidKeys { public, private };
            s.store().set_vapid_keys(&keys).await?;
            keys
        }
    };
    Ok(Json(VapidKeyResp {
        public_key: keys.public,
    }))
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
    auth: String,
}

fn parse_notif(s: &str) -> Result<NotificationId, ApiError> {
    Uuid::from_str(s)
        .map(NotificationId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid notification id".into()))
}

async fn subscribe(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Json(input): Json<SubscribeInput>,
) -> Result<StatusCode, ApiError> {
    if input.endpoint.is_empty() || input.keys.p256dh.is_empty() || input.keys.auth.is_empty() {
        return Err(ApiError::BadRequest("endpoint + keys required".into()));
    }
    let nid = parse_notif(&input.notification_id)?;
    // Org-gate the target channel: binding a browser subscription to
    // another org's notification row 404s here (cross-org IDOR).
    s.store().get_notification(nid, org.org_id).await?;
    s.store()
        .upsert_webpush_sub(nid, &input.endpoint, &input.keys.p256dh, &input.keys.auth)
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
    s.store()
        .delete_webpush_sub_by_endpoint(&input.endpoint)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
