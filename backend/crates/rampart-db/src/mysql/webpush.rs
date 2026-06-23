//! MySQL `webpush` domain — Web Push subscriptions + the shared VAPID keypair.
//! Ported from PG. Subscriptions key on `endpoint` (UNIQUE) so the upsert dedups
//! a re-subscribe via `ON DUPLICATE KEY UPDATE`. The VAPID keypair is NOT a table
//! — it lives in `settings` under `webpush_vapid` (via `mysql::settings`).
//! `get_or_create_vapid` stays a free fn (generic closure → not object-safe);
//! the Store composes get_vapid/set_vapid around its own generator.

use crate::webpush::{VapidKeys, WebpushSubscription};
use crate::DbResult;
use rampart_core::ids::NotificationId;
use sqlx::{MySqlPool, Row};
use uuid::Uuid;

pub async fn list_for_notification(
    pool: &MySqlPool,
    notification: NotificationId,
) -> DbResult<Vec<WebpushSubscription>> {
    let rows = sqlx::query(
        "SELECT id, endpoint, p256dh, auth FROM webpush_subscriptions WHERE notification_id = ?",
    )
    .bind(notification.0.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| WebpushSubscription {
            id: super::raw_uuid(&r.get::<String, _>("id")),
            endpoint: r.get("endpoint"),
            p256dh: r.get("p256dh"),
            auth: r.get("auth"),
        })
        .collect())
}

/// Upsert a subscription, keyed on `endpoint` (a re-subscribe refreshes keys).
pub async fn upsert(
    pool: &MySqlPool,
    notification: NotificationId,
    endpoint: &str,
    p256dh: &str,
    auth: &str,
) -> DbResult<()> {
    sqlx::query(
        "INSERT INTO webpush_subscriptions (id, notification_id, endpoint, p256dh, auth)
         VALUES (?, ?, ?, ?, ?)
         ON DUPLICATE KEY UPDATE
            notification_id = VALUES(notification_id),
            p256dh          = VALUES(p256dh),
            auth            = VALUES(auth)",
    )
    .bind(Uuid::now_v7().to_string())
    .bind(notification.0.to_string())
    .bind(endpoint)
    .bind(p256dh)
    .bind(auth)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_by_endpoint(pool: &MySqlPool, endpoint: &str) -> DbResult<()> {
    sqlx::query("DELETE FROM webpush_subscriptions WHERE endpoint = ?")
        .bind(endpoint)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete(pool: &MySqlPool, id: Uuid) -> DbResult<()> {
    sqlx::query("DELETE FROM webpush_subscriptions WHERE id = ?")
        .bind(id.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

/// Read the stored VAPID keypair (corrupt/absent → None so the caller regens).
pub async fn get_vapid(pool: &MySqlPool) -> DbResult<Option<VapidKeys>> {
    if let Some(v) = super::settings::get_setting(pool, "webpush_vapid").await? {
        if let Ok(keys) = serde_json::from_value::<VapidKeys>(v) {
            return Ok(Some(keys));
        }
    }
    Ok(None)
}

pub async fn set_vapid(pool: &MySqlPool, keys: &VapidKeys) -> DbResult<()> {
    let value = serde_json::to_value(keys).expect("serialize vapid keys");
    super::settings::put_setting(pool, "webpush_vapid", &value).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn subs_upsert_dedup_and_vapid(pool: MySqlPool) {
        let n = NotificationId::new();
        upsert(&pool, n, "https://push.example/abc", "k1", "a1")
            .await
            .unwrap();
        assert_eq!(list_for_notification(&pool, n).await.unwrap().len(), 1);

        // re-subscribe same endpoint → still 1, keys refreshed.
        upsert(&pool, n, "https://push.example/abc", "k2", "a2")
            .await
            .unwrap();
        let subs = list_for_notification(&pool, n).await.unwrap();
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].p256dh, "k2");

        // delete by endpoint.
        delete_by_endpoint(&pool, "https://push.example/abc")
            .await
            .unwrap();
        assert!(list_for_notification(&pool, n).await.unwrap().is_empty());

        // delete by id (no-op when absent — just doesn't error).
        delete(&pool, Uuid::now_v7()).await.unwrap();

        // vapid roundtrip via settings; corrupt/absent → None.
        assert!(get_vapid(&pool).await.unwrap().is_none());
        let keys = VapidKeys {
            public: "pub".into(),
            private: "priv".into(),
        };
        set_vapid(&pool, &keys).await.unwrap();
        let got = get_vapid(&pool).await.unwrap().unwrap();
        assert_eq!(got.public, "pub");
        assert_eq!(got.private, "priv");
    }
}
