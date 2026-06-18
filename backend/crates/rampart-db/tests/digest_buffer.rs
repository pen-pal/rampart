//! Integration test for the durable digest buffer across a simulated
//! restart.
//!
//! The notifier's digest coalescing buffers events in the `digest_buffer`
//! table (the source of truth) so a process restart resumes them instead
//! of dropping them. This test proves that contract at the DB layer:
//!
//!   1. Create a channel with a digest window.
//!   2. Enqueue several events into its buffer (the pre-restart state).
//!   3. Drop every in-memory handle and re-derive state purely from the DB
//!      — i.e. exactly what the flush task does on a fresh boot: call
//!      `drain_due` to find channels past their window, then
//!      `take_for_channel` to pull the buffered rows.
//!   4. Assert the events enqueued before the "restart" are all returned,
//!      in order, and that deleting the drained ids clears the buffer.
//!
//! The window start is `MIN(enqueued_at)`; to make the channel due without
//! sleeping, we use a 0-length window is not allowed (drain_due requires
//! digest_window_secs > 0), so we use a 1-second window and pass a `now`
//! comfortably in the future to the time-based `drain_due` query.

use rampart_core::{ChannelKind, NotificationId};
use rampart_db::digest_buffer;
use rampart_db::notifications::{create, NewNotification};
use sqlx::PgPool;
use time::OffsetDateTime;

const TEST_ORG: rampart_core::ids::OrgId =
    rampart_core::ids::OrgId::from_uuid(rampart_core::org::DEFAULT_ORG_ID);

fn digest_channel(name: &str, window_secs: i32) -> NewNotification {
    NewNotification {
        kind: ChannelKind::Webhook,
        name: name.into(),
        config: serde_json::json!({ "url": "https://example.com/hook" }),
        active: true,
        template_id: None,
        cooldown_seconds: 0,
        digest_window_secs: window_secs,
        quiet_hours_start: None,
        quiet_hours_end: None,
        rate_limit_per_hour: 0,
    }
}

/// Build a JSON blob standing in for a serialized notifier `Event`. The
/// digest_buffer layer treats it opaquely, so any shape round-trips.
fn event_json(label: &str) -> serde_json::Value {
    serde_json::json!({ "label": label, "kind": "status_flip" })
}

#[sqlx::test(migrations = "../../migrations")]
async fn buffered_events_survive_restart(pool: PgPool) {
    // 1. A channel with a 1-second digest window.
    let chan = create(&pool, digest_channel("digest-restart", 1), TEST_ORG)
        .await
        .expect("create channel");
    let id: NotificationId = chan.id;

    // 2. Enqueue three events — the pre-restart buffer.
    for label in ["first", "second", "third"] {
        digest_buffer::enqueue(&pool, id, &event_json(label))
            .await
            .expect("enqueue");
    }

    // Sanity: nothing else picks them up before the simulated restart.
    let pre = digest_buffer::take_for_channel(&pool, id)
        .await
        .expect("take pre-restart");
    assert_eq!(pre.len(), 3, "all three events buffered before restart");

    // 3. Simulate the restart: discard every in-memory value and re-derive
    //    state purely from the DB, as the flush task does on a fresh boot.
    //    Pass a `now` past the 1-second window so the channel is due.
    drop(pre);
    let now = OffsetDateTime::now_utc() + time::Duration::seconds(5);
    let due = digest_buffer::drain_due(&pool, now)
        .await
        .expect("drain_due after restart");
    assert!(
        due.iter().any(|c| c.notification_id == id),
        "channel is due to flush after restart",
    );

    // 4. The buffered events are recovered, oldest-first, intact.
    let rows = digest_buffer::take_for_channel(&pool, id)
        .await
        .expect("take_for_channel after restart");
    let labels: Vec<String> = rows
        .iter()
        .map(|r| {
            r.event_json["label"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    assert_eq!(
        labels,
        vec!["first", "second", "third"],
        "events recovered in enqueue order after restart",
    );

    // Draining (delete by the exact ids) clears the buffer, mirroring a
    // successful flush.
    let ids: Vec<_> = rows.iter().map(|r| r.id).collect();
    digest_buffer::delete_by_ids(&pool, &ids)
        .await
        .expect("delete_by_ids");
    let after = digest_buffer::take_for_channel(&pool, id)
        .await
        .expect("take after flush");
    assert!(after.is_empty(), "buffer empty after flush");
}
