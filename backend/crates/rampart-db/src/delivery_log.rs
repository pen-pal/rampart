//! Notification delivery log — append-only record of every channel send
//! attempt the notifier makes (success or failure).
//!
//! Append fits a single INSERT; reads use the descending `sent_at` index
//! plus an optional `before_ts` cursor so the UI can paginate the same way
//! the audit log does. Writes are best-effort from the notifier — a logging
//! failure must never break dispatch — so this module never bubbles errors
//! into the send path (the caller decides to swallow them).

use crate::{DbPool, DbResult};
use rampart_core::ids::{NotificationId, OrgId};
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

/// Append one delivery attempt and return the persisted row. Best-effort by
/// contract — callers in the notifier swallow the error so a logging failure
/// can't break dispatch. The returned [`DeliveryEntry`] lets the retry path
/// hand the freshly-recorded attempt straight back to the API caller.
pub async fn record(pool: &DbPool, entry: NewDelivery<'_>) -> DbResult<DeliveryEntry> {
    // Derive org_id from the referenced notification so the row is filed under
    // the same org as the channel it was sent through. When notification_id is
    // NULL (system event) or the channel was deleted (FK is ON DELETE SET NULL,
    // so the subselect yields no row), fall back to the Default org — without
    // this the Phase-3 read filter on delivery_log would make the row invisible.
    let row = sqlx::query!(
        r#"
        INSERT INTO delivery_log
            (notification_id, channel_kind, event_kind, monitor_id, ok, error, org_id)
        VALUES (
            $1, $2, $3, $4, $5, $6,
            COALESCE((SELECT org_id FROM notifications WHERE id = $1), $7::uuid)
        )
        RETURNING id, notification_id, channel_kind, event_kind,
                  monitor_id, ok, error, sent_at
        "#,
        entry.notification_id.map(|n| n.0),
        entry.channel_kind,
        entry.event_kind,
        entry.monitor_id,
        entry.ok,
        entry.error,
        rampart_core::org::DEFAULT_ORG_ID,
    )
    .fetch_one(pool)
    .await?;
    Ok(DeliveryEntry {
        id: row.id,
        notification_id: row.notification_id.map(NotificationId::from_uuid),
        channel_kind: row.channel_kind,
        event_kind: row.event_kind,
        monitor_id: row.monitor_id,
        ok: row.ok,
        error: row.error,
        sent_at: row.sent_at,
    })
}

/// Load a single delivery attempt by id. Returns `None` when no row matches
/// (the retry route turns that into a 404). The full row carries the
/// `notification_id` the retry path needs to re-resolve the channel.
pub async fn get(pool: &DbPool, id: i64, org_id: OrgId) -> DbResult<Option<DeliveryEntry>> {
    let row = sqlx::query!(
        r#"
        SELECT id, notification_id, channel_kind, event_kind,
               monitor_id, ok, error, sent_at
        FROM delivery_log
        WHERE id = $1 AND org_id = $2
        "#,
        id,
        org_id.0,
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| DeliveryEntry {
        id: r.id,
        notification_id: r.notification_id.map(NotificationId::from_uuid),
        channel_kind: r.channel_kind,
        event_kind: r.event_kind,
        monitor_id: r.monitor_id,
        ok: r.ok,
        error: r.error,
        sent_at: r.sent_at,
    }))
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
    org_id: OrgId,
) -> DbResult<Vec<DeliveryEntry>> {
    let limit = limit.clamp(1, 500);
    let rows = sqlx::query!(
        r#"
        SELECT id, notification_id, channel_kind, event_kind,
               monitor_id, ok, error, sent_at
        FROM delivery_log
        WHERE org_id = $3
          AND ($1::timestamptz IS NULL OR sent_at < $1)
        ORDER BY sent_at DESC, id DESC
        LIMIT $2
        "#,
        before_ts,
        limit,
        org_id.0,
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

/// Fetch up to `limit` deliveries newest-first for a bulk CSV export.
///
/// Unlike [`list`], this skips the keyset cursor and the 500-row clamp: an
/// export is a single full dump, so the caller passes its own (larger) cap.
/// `limit` is still floored at 1 to keep the query well-formed.
pub async fn list_all(pool: &DbPool, limit: i64, org_id: OrgId) -> DbResult<Vec<DeliveryEntry>> {
    let limit = limit.max(1);
    let rows = sqlx::query!(
        r#"
        SELECT id, notification_id, channel_kind, event_kind,
               monitor_id, ok, error, sent_at
        FROM delivery_log
        WHERE org_id = $2
        ORDER BY sent_at DESC, id DESC
        LIMIT $1
        "#,
        limit,
        org_id.0,
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
