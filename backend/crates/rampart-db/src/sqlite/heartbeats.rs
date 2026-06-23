//! SQLite `heartbeats` domain — the probe time-series. Mirrors a CORE subset of
//! the Postgres `crate::heartbeats` surface: `insert_many` (the writer),
//! `recent_for_monitor` (the detail feed), and `uptime_pct` (a simple
//! COUNT-ratio). DEFERRED: the analytic aggregations (summary_window, mtbf_mttr,
//! monthly_uptime, percentile latency, error-budget, daily-status batches) —
//! those lean on PG window/percentile functions and need app-side compute on
//! SQLite (the plan's dialect-divergent work).
//!
//! No UNNEST on SQLite, so `insert_many` loops per-row inside one transaction.
//! `ts` is INTEGER unix-seconds; status is TEXT; `important` is INTEGER 0/1.

use super::{mid, mstatus_from, mstatus_str, ts};
use crate::DbResult;
use rampart_core::ids::MonitorId;
use rampart_core::Heartbeat;
use sqlx::{Row, SqlitePool};

fn hb_from(r: &sqlx::sqlite::SqliteRow) -> Heartbeat {
    Heartbeat {
        monitor_id: mid(&r.get::<String, _>("monitor_id")),
        ts: ts(r.get::<i64, _>("ts")),
        status: mstatus_from(&r.get::<String, _>("status")),
        latency_ms: r.get("latency_ms"),
        status_code: r.get("status_code"),
        msg: r.get("msg"),
        retries: r.get("retries"),
        important: r.get::<i64, _>("important") != 0,
    }
}

/// Bulk-insert heartbeats (one tx, per-row INSERT; PG uses UNNEST). Idempotent
/// on `(monitor_id, ts)` via `ON CONFLICT DO NOTHING`.
pub async fn insert_many(pool: &SqlitePool, hbs: &[Heartbeat]) -> DbResult<()> {
    if hbs.is_empty() {
        return Ok(());
    }
    let mut tx = pool.begin().await?;
    for h in hbs {
        sqlx::query(
            "INSERT INTO heartbeats
                (monitor_id, ts, status, latency_ms, status_code, msg, retries, important)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(monitor_id, ts) DO NOTHING",
        )
        .bind(h.monitor_id.0.to_string())
        .bind(h.ts.unix_timestamp())
        .bind(mstatus_str(h.status))
        .bind(h.latency_ms)
        .bind(h.status_code)
        .bind(&h.msg)
        .bind(h.retries)
        .bind(h.important as i64)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Most-recent heartbeats for a monitor, newest first.
pub async fn recent_for_monitor(
    pool: &SqlitePool,
    monitor: MonitorId,
    limit: i64,
) -> DbResult<Vec<Heartbeat>> {
    let rows = sqlx::query(
        "SELECT monitor_id, ts, status, latency_ms, status_code, msg, retries, important
         FROM heartbeats WHERE monitor_id = ? ORDER BY ts DESC LIMIT ?",
    )
    .bind(monitor.0.to_string())
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(hb_from).collect())
}

/// Uptime percentage over the trailing `window_seconds`. `None` when the window
/// holds zero heartbeats (caller renders "no data" rather than a misleading 0%).
pub async fn uptime_pct(
    pool: &SqlitePool,
    monitor: MonitorId,
    window_seconds: i64,
) -> DbResult<Option<f64>> {
    let since = time::OffsetDateTime::now_utc().unix_timestamp() - window_seconds;
    let (total, ok): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(CASE WHEN status = 'up' THEN 1 ELSE 0 END), 0)
         FROM heartbeats WHERE monitor_id = ? AND ts >= ?",
    )
    .bind(monitor.0.to_string())
    .bind(since)
    .fetch_one(pool)
    .await?;
    if total == 0 {
        return Ok(None);
    }
    Ok(Some(ok as f64 / total as f64 * 100.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::monitors;
    use rampart_core::monitor::{MonitorKind, MonitorStatus, NewMonitor};
    use sqlx::SqlitePool;
    use time::OffsetDateTime;

    async fn monitor(pool: &SqlitePool) -> MonitorId {
        let org = super::super::oid("00000000-0000-0000-0000-000000000001");
        monitors::create(
            pool,
            NewMonitor {
                name: "m".into(),
                kind: MonitorKind::Http,
                url: Some("https://x".into()),
                hostname: None,
                port: None,
                config: serde_json::json!({}),
                interval_seconds: 60,
                timeout_seconds: 10,
                max_retries: 0,
                retry_interval_sec: 60,
                resend_interval_sec: 0,
                upside_down: false,
                http_method: "GET".into(),
                http_body: None,
                http_headers: None,
                accepted_statuses: vec![200],
                follow_redirect: true,
                ignore_tls: false,
                proxy_id: None,
                group_id: None,
                check_cert: false,
                cert_expiry_days: 14,
                slo_target_pct: None,
                slo_window_days: None,
                agent_id: None,
                escalation_policy_id: None,
            },
            org,
        )
        .await
        .unwrap()
        .id
    }

    fn hb(m: MonitorId, secs_ago: i64, status: MonitorStatus) -> Heartbeat {
        Heartbeat {
            monitor_id: m,
            ts: OffsetDateTime::from_unix_timestamp(
                OffsetDateTime::now_utc().unix_timestamp() - secs_ago,
            )
            .unwrap(),
            status,
            latency_ms: Some(42),
            status_code: Some(200),
            msg: None,
            retries: 0,
            important: false,
        }
    }

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn insert_recent_and_uptime(pool: SqlitePool) {
        let m = monitor(&pool).await;
        // 3 up, 1 down over the last few minutes.
        insert_many(
            &pool,
            &[
                hb(m, 10, MonitorStatus::Up),
                hb(m, 20, MonitorStatus::Up),
                hb(m, 30, MonitorStatus::Down),
                hb(m, 40, MonitorStatus::Up),
            ],
        )
        .await
        .unwrap();

        let recent = recent_for_monitor(&pool, m, 10).await.unwrap();
        assert_eq!(recent.len(), 4);
        // Newest first (10s ago) is Up, with the bound latency.
        assert_eq!(recent[0].status, MonitorStatus::Up);
        assert_eq!(recent[0].latency_ms, Some(42));

        // 3/4 up = 75% over the last hour.
        let up = uptime_pct(&pool, m, 3600).await.unwrap().unwrap();
        assert!((up - 75.0).abs() < 0.001, "uptime {up}");

        // Idempotent on (monitor_id, ts): re-insert is a no-op.
        insert_many(&pool, &[hb(m, 10, MonitorStatus::Up)])
            .await
            .unwrap();
        assert_eq!(recent_for_monitor(&pool, m, 10).await.unwrap().len(), 4);

        // No data in a tiny recent window → None.
        assert!(uptime_pct(&pool, m, 1).await.unwrap().is_none());
    }
}
