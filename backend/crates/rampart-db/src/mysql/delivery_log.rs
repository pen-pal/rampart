//! MySQL `delivery_log` domain — append-only record of every channel send.
//! Mirrors the PG/SQLite surface (record / get / list / list_all). MySQL deltas:
//! no `RETURNING` → INSERT then re-SELECT by `LAST_INSERT_ID()`; `ok`→TINYINT,
//! `sent_at`→BIGINT. `record` floors org to the channel's org (or Default) in-SQL
//! so a system/orphaned row is never NULL.

use super::{default_org_id_str, raw_uuid, ts};
use crate::delivery_log::{DeliveryEntry, NewDelivery};
use crate::DbResult;
use rampart_core::ids::{NotificationId, OrgId};
use sqlx::{MySqlPool, Row};
use time::OffsetDateTime;
use uuid::Uuid;

fn entry_from(r: &sqlx::mysql::MySqlRow) -> DeliveryEntry {
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

/// Append one delivery attempt; org floored to the channel's org (or Default).
/// No RETURNING — re-select the row by the auto-increment id.
pub async fn record(pool: &MySqlPool, entry: NewDelivery<'_>) -> DbResult<DeliveryEntry> {
    let notif = entry.notification_id.map(|n| n.0.to_string());
    let res = sqlx::query(
        "INSERT INTO delivery_log
           (notification_id, channel_kind, event_kind, monitor_id, ok, error, org_id)
         VALUES (?, ?, ?, ?, ?, ?,
                 COALESCE((SELECT org_id FROM notifications WHERE id = ?), ?))",
    )
    .bind(notif.clone())
    .bind(entry.channel_kind)
    .bind(entry.event_kind)
    .bind(entry.monitor_id.map(|m| m.to_string()))
    .bind(entry.ok as i64)
    .bind(entry.error)
    .bind(notif)
    .bind(default_org_id_str())
    .execute(pool)
    .await?;
    let id = res.last_insert_id() as i64;
    let sql = format!("SELECT {COLS} FROM delivery_log WHERE id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_one(pool)
        .await?;
    Ok(entry_from(&row))
}

pub async fn get(pool: &MySqlPool, id: i64, org_id: OrgId) -> DbResult<Option<DeliveryEntry>> {
    let sql = format!("SELECT {COLS} FROM delivery_log WHERE id = ? AND org_id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(org_id.0.to_string())
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| entry_from(&r)))
}

#[allow(clippy::too_many_arguments)]
pub async fn list(
    pool: &MySqlPool,
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

pub async fn list_all(pool: &MySqlPool, limit: i64, org_id: OrgId) -> DbResult<Vec<DeliveryEntry>> {
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
    use crate::mysql::notifications;
    use crate::notifications::NewNotification;
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

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn record_org_floor_and_get(pool: MySqlPool) {
        let org = super::super::oid(DEF);
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

        let sys = record(&pool, nd(None, "result_webhook", None, false, Some("boom")))
            .await
            .unwrap();
        assert!(!sys.ok);
        assert_eq!(sys.error.as_deref(), Some("boom"));
        assert!(get(&pool, sys.id, org).await.unwrap().is_some());
    }

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn list_filter_matrix_and_limit(pool: MySqlPool) {
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

        assert_eq!(
            list(&pool, 100, None, None, None, None, org)
                .await
                .unwrap()
                .len(),
            3
        );
        assert_eq!(
            list(&pool, 100, None, Some(false), None, None, org)
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            list(&pool, 100, None, None, None, Some("slack"), org)
                .await
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            list(&pool, 100, None, None, Some(mon), None, org)
                .await
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            list(&pool, 100, None, None, Some(mon), Some("slack"), org)
                .await
                .unwrap()
                .len(),
            1
        );
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
