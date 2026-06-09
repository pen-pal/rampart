//! Durable backing store for the notifier's per-channel digest buffer.
//!
//! When a channel has `digest_window_secs > 0` the notifier coalesces
//! events into one combined message per window instead of firing each
//! immediately. The buffered events are persisted here so a restart
//! resumes them rather than dropping them — the DB is the source of
//! truth; the notifier keeps an in-memory write-through cache only as an
//! optimisation.
//!
//! One row per buffered event. The combined message is rendered at flush
//! time from every row sharing a `notification_id`.

use crate::{DbPool, DbResult};
use rampart_core::ids::NotificationId;
use time::OffsetDateTime;
use uuid::Uuid;

/// A persisted buffered event plus its DB row id (needed to delete the
/// exact rows that were drained for a flush, so concurrently-enqueued
/// events landing mid-flush aren't lost).
#[derive(Debug, Clone)]
pub struct BufferedEvent {
    pub id: Uuid,
    pub event_json: serde_json::Value,
}

/// A channel whose oldest buffered event is now older than its configured
/// digest window — i.e. it's due to flush.
#[derive(Debug, Clone)]
pub struct DueChannel {
    pub notification_id: NotificationId,
}

/// Persist one buffered event for `notification_id`. `enqueued_at` defaults
/// to NOW() — the per-channel MIN of these is the window start.
pub async fn enqueue(
    pool: &DbPool,
    notification_id: NotificationId,
    event_json: &serde_json::Value,
) -> DbResult<()> {
    let id = Uuid::now_v7();
    sqlx::query!(
        r#"
        INSERT INTO digest_buffer (id, notification_id, event_json)
        VALUES ($1, $2, $3)
        "#,
        id,
        notification_id.0,
        event_json,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Channels whose oldest buffered event has aged past that channel's
/// `digest_window_secs`. Joins the buffer to `notifications` so each
/// channel is gated by its own window; channels still inside their window
/// are excluded. The notifier flushes exactly the ids returned here.
pub async fn drain_due(pool: &DbPool, now: OffsetDateTime) -> DbResult<Vec<DueChannel>> {
    let rows = sqlx::query!(
        r#"
        SELECT db.notification_id AS "notification_id!"
        FROM digest_buffer db
        JOIN notifications n ON n.id = db.notification_id
        WHERE n.digest_window_secs > 0
        GROUP BY db.notification_id, n.digest_window_secs
        HAVING MIN(db.enqueued_at) <= $1::timestamptz - make_interval(secs => n.digest_window_secs)
        "#,
        now,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| DueChannel {
            notification_id: NotificationId::from_uuid(r.notification_id),
        })
        .collect())
}

/// All buffered events for one channel, oldest first, with their row ids.
/// The caller renders the combined message from these then calls
/// [`delete_by_ids`] with the returned `id`s.
pub async fn take_for_channel(
    pool: &DbPool,
    notification_id: NotificationId,
) -> DbResult<Vec<BufferedEvent>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, event_json
        FROM digest_buffer
        WHERE notification_id = $1
        ORDER BY enqueued_at ASC, id ASC
        "#,
        notification_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| BufferedEvent {
            id: r.id,
            event_json: r.event_json,
        })
        .collect())
}

/// Delete the exact buffer rows that were drained for a flush. Scoped to
/// the row ids (not the whole channel) so events enqueued after the drain
/// snapshot survive into the next window.
pub async fn delete_by_ids(pool: &DbPool, ids: &[Uuid]) -> DbResult<()> {
    if ids.is_empty() {
        return Ok(());
    }
    sqlx::query!(r#"DELETE FROM digest_buffer WHERE id = ANY($1)"#, ids,)
        .execute(pool)
        .await?;
    Ok(())
}
