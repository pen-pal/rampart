//! SQLite `delivery_log` domain — append-only record of every channel send.
//! Mirrors the Postgres `crate::delivery_log` free fns: record / get / list /
//! list_all. Structs (`DeliveryEntry`, `NewDelivery`) are reused from the PG
//! module (dialect-neutral).
//!
//! Dialect: `BIGSERIAL` PK → `INTEGER PRIMARY KEY` (rowid alias, read as i64);
//! `sent_at` INTEGER unix-seconds; `ok` INTEGER 0/1; uuids TEXT. `record`
//! derives org in-SQL via `COALESCE((SELECT org_id FROM notifications WHERE
//! id = ?), <Default>)` — the same floor PG uses so a system/orphaned row is
//! never NULL (Phase-3 read filter would hide it). SQLite's sqlx binder is
//! positional, so a value referenced by two `?` placeholders is bound twice.

use super::{default_org_id_str, raw_uuid, ts};
use crate::delivery_log::{DeliveryEntry, NewDelivery};
use crate::DbResult;
use rampart_core::ids::{NotificationId, OrgId};
use sqlx::{Row, SqlitePool};
use time::OffsetDateTime;
use uuid::Uuid;

fn entry_from(r: &sqlx::sqlite::SqliteRow) -> DeliveryEntry {
    DeliveryEntry {
        id: r.get::<i64, _>("id"),
        notification_id: r
            .get::<Option<String>, _>("notification_id")
            .map(|s| NotificationId::from_uuid(raw_uuid(&s))),
        channel_kind: r.get("channel_kind"),
        event_kind: r.get("event_kind"),
        monitor_id: r
            .get::<Option<String>, _>("monitor_id")
            .map(|s| raw_uuid(&s)),
        ok: r.get::<i64, _>("ok") != 0,
        error: r.get("error"),
        sent_at: ts(r.get::<i64, _>("sent_at")),
    }
}

const COLS: &str = "id, notification_id, channel_kind, event_kind, monitor_id, ok, error, sent_at";

/// Append one delivery attempt; org is floored to the channel's org (or Default
/// when unlinked). Best-effort by contract — callers swallow the error.
pub async fn record(pool: &SqlitePool, entry: NewDelivery<'_>) -> DbResult<DeliveryEntry> {
    let notif = entry.notification_id.map(|n| n.0.to_string());
    let row = sqlx::query(
        "INSERT INTO delivery_log
           (notification_id, channel_kind, event_kind, monitor_id, ok, error, org_id)
         VALUES (?, ?, ?, ?, ?, ?,
                 COALESCE((SELECT org_id FROM notifications WHERE id = ?), ?))
         RETURNING id, notification_id, channel_kind, event_kind, monitor_id, ok, error, sent_at",
    )
    .bind(notif.clone()) // column value
    .bind(entry.channel_kind)
    .bind(entry.event_kind)
    .bind(entry.monitor_id.map(|m| m.to_string()))
    .bind(entry.ok as i64)
    .bind(entry.error)
    .bind(notif) // subselect lookup (positional binder → bind twice)
    .bind(default_org_id_str())
    .fetch_one(pool)
    .await?;
    Ok(entry_from(&row))
}

/// One entry scoped to an org (`None` → 404 in the route). Carries the
/// notification_id needed to re-resolve the channel on retry.
pub async fn get(pool: &SqlitePool, id: i64, org_id: OrgId) -> DbResult<Option<DeliveryEntry>> {
    let sql = format!("SELECT {COLS} FROM delivery_log WHERE id = ? AND org_id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(org_id.0.to_string())
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| entry_from(&r)))
}

/// Keyset page, newest-first, with optional filters. `limit` clamped 1..=500.
pub async fn list(
    pool: &SqlitePool,
    limit: i64,
    before_ts: Option<OffsetDateTime>,
    ok: Option<bool>,
    monitor: Option<Uuid>,
    channel: Option<&str>,
    org_id: OrgId,
) -> DbResult<Vec<DeliveryEntry>> {
    let limit = limit.clamp(1, 500);
    let before = before_ts.map(|t| t.unix_timestamp());
    let ok_i = ok.map(|b| b as i64);
    let mon = monitor.map(|m| m.to_string());
    let sql = format!(
        "SELECT {COLS} FROM delivery_log
         WHERE org_id = ?
           AND (? IS NULL OR sent_at < ?)
           AND (? IS NULL OR ok = ?)
           AND (? IS NULL OR monitor_id = ?)
           AND (? IS NULL OR channel_kind = ?)
         ORDER BY sent_at DESC, id DESC
         LIMIT ?"
    );
    // Each nullable filter is referenced twice; positional binder → bind twice.
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(org_id.0.to_string())
        .bind(before)
        .bind(before)
        .bind(ok_i)
        .bind(ok_i)
        .bind(mon.clone())
        .bind(mon)
        .bind(channel)
        .bind(channel)
        .bind(limit)
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(entry_from).collect())
}

/// Full org dump for CSV export — no keyset cursor, no 500 clamp (caller caps).
pub async fn list_all(
    pool: &SqlitePool,
    limit: i64,
    org_id: OrgId,
) -> DbResult<Vec<DeliveryEntry>> {
    let limit = limit.max(1);
    let sql = format!(
        "SELECT {COLS} FROM delivery_log WHERE org_id = ? ORDER BY sent_at DESC, id DESC LIMIT ?"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(org_id.0.to_string())
        .bind(limit)
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(entry_from).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notifications::NewNotification;
    use crate::sqlite::notifications;
    use rampart_core::ChannelKind;

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    fn a_channel(name: &str) -> NewNotification {
        NewNotification {
            kind: ChannelKind::Slack,
            name: name.into(),
            config: serde_json::json!({ "url": "https://hooks.example/x" }),
            active: true,
            template_id: None,
            cooldown_seconds: 0,
            digest_window_secs: 0,
            quiet_hours_start: None,
            quiet_hours_end: None,
            rate_limit_per_hour: 0,
        }
    }

    fn nd<'a>(
        notif: Option<NotificationId>,
        channel: &'a str,
        monitor: Option<Uuid>,
        ok: bool,
        error: Option<&'a str>,
    ) -> NewDelivery<'a> {
        NewDelivery {
            notification_id: notif,
            channel_kind: channel,
            event_kind: "status_flip",
            monitor_id: monitor,
            ok,
            error,
        }
    }

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn record_org_floor_and_get(pool: SqlitePool) {
        let org = super::super::oid(DEF);
        // Linked to a real channel → inherits that channel's org.
        let chan = notifications::create(&pool, a_channel("slack"), org)
            .await
            .unwrap();
        let linked = record(&pool, nd(Some(chan.id), "slack", None, true, None))
            .await
            .unwrap();
        assert!(linked.ok);
        assert_eq!(
            get(&pool, linked.id, org).await.unwrap().unwrap().id,
            linked.id
        );

        // Unlinked (system) row floors to Default org.
        let sys = record(&pool, nd(None, "result_webhook", None, false, Some("boom")))
            .await
            .unwrap();
        assert!(!sys.ok);
        assert_eq!(sys.error.as_deref(), Some("boom"));
        assert!(get(&pool, sys.id, org).await.unwrap().is_some()); // DEF == org here
    }

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn list_filter_matrix_and_limit(pool: SqlitePool) {
        let org = super::super::oid(DEF);
        let mon = Uuid::now_v7();
        let other = Uuid::now_v7();
        record(&pool, nd(None, "slack", Some(mon), true, None))
            .await
            .unwrap();
        record(&pool, nd(None, "webhook", Some(mon), false, Some("x")))
            .await
            .unwrap();
        record(&pool, nd(None, "slack", Some(other), true, None))
            .await
            .unwrap();

        let all = list(&pool, 100, None, None, None, None, org).await.unwrap();
        assert_eq!(all.len(), 3);
        // newest-first.
        let only_fail = list(&pool, 100, None, Some(false), None, None, org)
            .await
            .unwrap();
        assert_eq!(only_fail.len(), 1);
        let only_slack = list(&pool, 100, None, None, None, Some("slack"), org)
            .await
            .unwrap();
        assert_eq!(only_slack.len(), 2);
        let only_mon = list(&pool, 100, None, None, Some(mon), None, org)
            .await
            .unwrap();
        assert_eq!(only_mon.len(), 2);
        let slack_and_mon = list(&pool, 100, None, None, Some(mon), Some("slack"), org)
            .await
            .unwrap();
        assert_eq!(slack_and_mon.len(), 1);

        // limit clamps to >=1; list_all ignores the 500 ceiling but caps at limit.
        assert_eq!(
            list(&pool, 0, None, None, None, None, org)
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(list_all(&pool, 2, org).await.unwrap().len(), 2);
    }
}
