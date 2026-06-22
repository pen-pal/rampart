//! Web Push subscription storage + shared VAPID keypair.
//!
//! Subscriptions are created by the browser subscribe flow and attached
//! to a `webpush` notification row. The VAPID keypair is shared across
//! all webpush channels and persisted in `settings` under `webpush_vapid`.

use crate::{DbPool, DbResult};
use rampart_core::ids::NotificationId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct WebpushSubscription {
    pub id: Uuid,
    pub endpoint: String,
    pub p256dh: String,
    pub auth: String,
}

/// VAPID keypair as stored in settings. `private` is base64 PKCS#8 DER;
/// `public` is base64url uncompressed P-256 point (handed to the browser).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VapidKeys {
    pub public: String,
    pub private: String,
}

pub async fn list_for_notification(
    pool: &DbPool,
    notification: NotificationId,
) -> DbResult<Vec<WebpushSubscription>> {
    let rows = sqlx::query!(
        r#"SELECT id, endpoint, p256dh, auth
           FROM webpush_subscriptions
           WHERE notification_id = $1"#,
        notification.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| WebpushSubscription {
            id: r.id,
            endpoint: r.endpoint,
            p256dh: r.p256dh,
            auth: r.auth,
        })
        .collect())
}

/// Upsert a subscription. Re-subscribing from the same browser yields the
/// same endpoint, so we key on it and refresh the keys.
pub async fn upsert(
    pool: &DbPool,
    notification: NotificationId,
    endpoint: &str,
    p256dh: &str,
    auth: &str,
) -> DbResult<()> {
    sqlx::query!(
        r#"
        INSERT INTO webpush_subscriptions (id, notification_id, endpoint, p256dh, auth)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (endpoint) DO UPDATE
            SET notification_id = EXCLUDED.notification_id,
                p256dh          = EXCLUDED.p256dh,
                auth            = EXCLUDED.auth
        "#,
        Uuid::now_v7(),
        notification.0,
        endpoint,
        p256dh,
        auth,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_by_endpoint(pool: &DbPool, endpoint: &str) -> DbResult<()> {
    sqlx::query!(
        r#"DELETE FROM webpush_subscriptions WHERE endpoint = $1"#,
        endpoint,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete(pool: &DbPool, id: Uuid) -> DbResult<()> {
    sqlx::query!(r#"DELETE FROM webpush_subscriptions WHERE id = $1"#, id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Read the stored VAPID keypair, if one is persisted and parseable. A
/// corrupt blob reads as `None` so the caller regenerates. Object-safe
/// primitive (no closure) — seamed onto the `Store` trait.
pub async fn get_vapid(pool: &DbPool) -> DbResult<Option<VapidKeys>> {
    if let Some(v) = crate::settings::get(pool, "webpush_vapid").await? {
        if let Ok(keys) = serde_json::from_value::<VapidKeys>(v) {
            return Ok(Some(keys));
        }
        // Corrupt blob — treat as absent so the caller regenerates.
    }
    Ok(None)
}

/// Persist the shared VAPID keypair. Object-safe primitive — seamed onto
/// the `Store` trait.
pub async fn set_vapid(pool: &DbPool, keys: &VapidKeys) -> DbResult<()> {
    let value = serde_json::to_value(keys).expect("serialize vapid keys");
    crate::settings::put(pool, "webpush_vapid", &value).await
}

/// Fetch the shared VAPID keypair, generating + persisting one on first
/// call. `gen` lets the caller pass the key generator (lives in the
/// notifier crate so the crypto stays in one place). The generic closure
/// makes this non-object-safe, so it stays a free fn — `Store`-using
/// callers compose the two primitives ([`get_vapid`] / [`set_vapid`])
/// around their own generator instead.
pub async fn get_or_create_vapid(
    pool: &DbPool,
    gen: impl FnOnce() -> (String, String),
) -> DbResult<VapidKeys> {
    if let Some(keys) = get_vapid(pool).await? {
        return Ok(keys);
    }
    let (public, private) = gen();
    let keys = VapidKeys { public, private };
    set_vapid(pool, &keys).await?;
    Ok(keys)
}
