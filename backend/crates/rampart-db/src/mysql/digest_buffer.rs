//! MySQL `digest_buffer` domain — durable backing store for the notifier's
//! per-channel digest buffer. Mirrors the PG/SQLite free-fn surface (enqueue /
//! drain_due / take_for_channel / delete_by_ids), reusing
//! `crate::digest_buffer::{BufferedEvent, DueChannel}`.
//!
//! Dialect: uuid→CHAR(36), jsonb event_json→LONGTEXT, enqueued_at→BIGINT.
//! `drain_due` joins `notifications` so each channel is gated by its own
//! `digest_window_secs`, identical to PG/SQLite.

use super::{in_placeholders, raw_uuid};
use crate::digest_buffer::{BufferedEvent, DueChannel};
use crate::DbResult;
use rampart_core::ids::NotificationId;
use sqlx::{MySqlPool, Row};
use time::OffsetDateTime;
use uuid::Uuid;

/// Persist one buffered event for `notification_id` (enqueued_at = now).
pub async fn enqueue(
    pool: &MySqlPool,
    notification_id: NotificationId,
    event_json: &serde_json::Value,
) -> DbResult<()> {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO digest_buffer (id, notification_id, event_json) VALUES (?, ?, ?)")
        .bind(id.to_string())
        .bind(notification_id.0.to_string())
        .bind(serde_json::to_string(event_json).unwrap_or_else(|_| "null".into()))
        .execute(pool)
        .await?;
    Ok(())
}

/// Channels whose oldest buffered event has aged past their own
/// `digest_window_secs` — the set to flush this tick.
pub async fn drain_due(pool: &MySqlPool, now: OffsetDateTime) -> DbResult<Vec<DueChannel>> {
    let rows = sqlx::query(
        "SELECT db.notification_id AS notification_id
         FROM digest_buffer db
         JOIN notifications n ON n.id = db.notification_id
         WHERE n.digest_window_secs > 0
         GROUP BY db.notification_id, n.digest_window_secs
         HAVING MIN(db.enqueued_at) <= ? - n.digest_window_secs",
    )
    .bind(now.unix_timestamp())
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| DueChannel {
            notification_id: NotificationId::from_uuid(raw_uuid(
                &r.get::<String, _>("notification_id"),
            )),
        })
        .collect())
}

/// All buffered events for one channel, oldest first, with their row ids.
pub async fn take_for_channel(
    pool: &MySqlPool,
    notification_id: NotificationId,
) -> DbResult<Vec<BufferedEvent>> {
    let rows = sqlx::query(
        "SELECT id, event_json FROM digest_buffer
         WHERE notification_id = ? ORDER BY enqueued_at ASC, id ASC",
    )
    .bind(notification_id.0.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| BufferedEvent {
            id: raw_uuid(&r.get::<String, _>("id")),
            event_json: serde_json::from_str(&r.get::<String, _>("event_json"))
                .unwrap_or(serde_json::Value::Null),
        })
        .collect())
}

/// Delete the exact drained rows (scoped to ids, not the whole channel, so
/// events enqueued mid-flush survive into the next window).
pub async fn delete_by_ids(pool: &MySqlPool, ids: &[Uuid]) -> DbResult<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let sql = format!(
        "DELETE FROM digest_buffer WHERE id IN ({})",
        in_placeholders(ids.len())
    );
    let mut q = sqlx::query(sqlx::AssertSqlSafe(sql));
    for id in ids {
        q = q.bind(id.to_string());
    }
    q.execute(pool).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mysql::notifications;
    use crate::notifications::NewNotification;
    use time::Duration;

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn enqueue_drain_take_delete(pool: MySqlPool) {
        let org = super::super::oid(DEF);
        // a digest channel (window 60s).
        let n: NewNotification = serde_json::from_value(serde_json::json!({
            "kind": "webhook",
            "name": "ch",
            "config": { "url": "https://example.com/hook" },
            "digest_window_secs": 60
        }))
        .unwrap();
        let ch = notifications::create(&pool, n, org).await.unwrap();

        let ev = serde_json::json!({ "msg": "hi" });
        enqueue(&pool, ch.id, &ev).await.unwrap();
        enqueue(&pool, ch.id, &ev).await.unwrap();

        // now → events still inside the 60s window → not due.
        assert!(drain_due(&pool, OffsetDateTime::now_utc())
            .await
            .unwrap()
            .is_empty());

        // an hour from now → oldest event aged past the window → due once.
        let due = drain_due(&pool, OffsetDateTime::now_utc() + Duration::hours(1))
            .await
            .unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].notification_id, ch.id);

        // take both, scoped-delete by id, buffer empty after.
        let buf = take_for_channel(&pool, ch.id).await.unwrap();
        assert_eq!(buf.len(), 2);
        assert_eq!(buf[0].event_json, ev);
        let ids: Vec<Uuid> = buf.iter().map(|b| b.id).collect();
        delete_by_ids(&pool, &ids).await.unwrap();
        assert!(take_for_channel(&pool, ch.id).await.unwrap().is_empty());

        // empty-ids delete is a no-op.
        delete_by_ids(&pool, &[]).await.unwrap();
    }
}
