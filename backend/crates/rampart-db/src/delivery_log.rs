//! Notification delivery log — append-only record of every channel send
//! attempt the notifier makes (success or failure).
//!
//! Append fits a single INSERT; reads use the descending `sent_at` index
//! plus an optional `before_ts` cursor so the UI can paginate the same way
//! the audit log does. Writes are best-effort from the notifier — a logging
//! failure must never break dispatch — so this module never bubbles errors
//! into the send path (the caller decides to swallow them).

use crate::{DbPool, DbResult};
use rampart_core::ids::NotificationId;
use serde::Serialize;
use time::OffsetDateTime;
use uuid::Uuid;

/// One persisted delivery attempt, returned newest-first by [`list`].
#[derive(Debug, Clone, Serialize)]
pub struct DeliveryEntry {
    pub id: i64,
    pub notification_id: Option<NotificationId>,
    pub channel_kind: String,
    pub event_kind: String,
    pub monitor_id: Option<Uuid>,
    pub ok: bool,
    pub error: Option<String>,
    pub sent_at: OffsetDateTime,
}

/// A single delivery attempt to record. `channel_kind` / `event_kind` are
/// the snake_case string forms (denormalised so the row stays readable
/// after the channel is deleted — the FK is ON DELETE SET NULL).
pub struct NewDelivery<'a> {
    pub notification_id: Option<NotificationId>,
    pub channel_kind: &'a str,
    pub event_kind: &'a str,
    pub monitor_id: Option<Uuid>,
    pub ok: bool,
    pub error: Option<&'a str>,
}

/// Append one delivery attempt. Best-effort by contract — callers in the
/// notifier swallow the error so a logging failure can't break dispatch.
pub async fn record(pool: &DbPool, entry: NewDelivery<'_>) -> DbResult<()> {
    sqlx::query!(
        r#"
        INSERT INTO delivery_log
            (notification_id, channel_kind, event_kind, monitor_id, ok, error)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
        entry.notification_id.map(|n| n.0),
        entry.channel_kind,
        entry.event_kind,
        entry.monitor_id,
        entry.ok,
        entry.error,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// List recent deliveries, newest-first. `before_ts` is a keyset cursor:
/// pass `None` for the first page, then the `sent_at` of the last row of
/// the previous page. `limit` is clamped to a sane window. Ordered by
/// `(sent_at DESC, id DESC)` so the secondary id keeps the order total when
/// several rows share a timestamp.
pub async fn list(
    pool: &DbPool,
    limit: i64,
    before_ts: Option<OffsetDateTime>,
) -> DbResult<Vec<DeliveryEntry>> {
    let limit = limit.clamp(1, 500);
    let rows = sqlx::query!(
        r#"
        SELECT id, notification_id, channel_kind, event_kind,
               monitor_id, ok, error, sent_at
        FROM delivery_log
        WHERE ($1::timestamptz IS NULL OR sent_at < $1)
        ORDER BY sent_at DESC, id DESC
        LIMIT $2
        "#,
        before_ts,
        limit,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| DeliveryEntry {
            id: r.id,
            notification_id: r.notification_id.map(NotificationId::from_uuid),
            channel_kind: r.channel_kind,
            event_kind: r.event_kind,
            monitor_id: r.monitor_id,
            ok: r.ok,
            error: r.error,
            sent_at: r.sent_at,
        })
        .collect())
}
