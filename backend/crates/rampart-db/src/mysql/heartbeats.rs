//! MySQL `heartbeats` domain (CORE) — the probe time-series writer + the history
//! feeds + the trailing-window uptime/latency reads the scheduler + monitor
//! detail need. The dashboard rollups (daily/monthly buckets, mtbf/mttr, error
//! budget, the batch variants) land in a follow-up `heartbeats-analytics` slice.
//!
//! MySQL deltas vs SQLite: `ON CONFLICT DO NOTHING` → `ON DUPLICATE KEY`;
//! **`SUM(CASE…)` returns DECIMAL → `CAST(… AS SIGNED)` so it decodes as i64**;
//! `/` is decimal division → use `DIV` for integer buckets (in the analytics
//! slice); derived tables need an alias (`) AS sub`). ts→BIGINT, bool→TINYINT,
//! latency/status_code/retries i32→INT.

use super::{mid, mstatus_from, mstatus_str, ts};
use crate::DbResult;
use rampart_core::ids::{MonitorId, OrgId};
use rampart_core::Heartbeat;
use sqlx::{MySqlPool, Row};
use time::OffsetDateTime;

fn since_secs(window_seconds: i64) -> i64 {
    OffsetDateTime::now_utc().unix_timestamp() - window_seconds
}

fn hb_from(r: &sqlx::mysql::MySqlRow) -> Heartbeat {
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

/// Bulk-insert heartbeats (one tx, per-row INSERT). Idempotent on
/// `(monitor_id, ts)` via `ON DUPLICATE KEY`.
pub async fn insert_many(pool: &MySqlPool, hbs: &[Heartbeat]) -> DbResult<()> {
    if hbs.is_empty() {
        return Ok(());
    }
    let mut tx = pool.begin().await?;
    for h in hbs {
        sqlx::query(
            "INSERT INTO heartbeats
                (monitor_id, ts, status, latency_ms, status_code, msg, retries, important)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON DUPLICATE KEY UPDATE monitor_id = monitor_id",
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

pub async fn recent_for_monitor(
    pool: &MySqlPool,
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

pub async fn recent_for_monitor_before(
    pool: &MySqlPool,
    monitor: MonitorId,
    limit: i64,
    before: Option<OffsetDateTime>,
) -> DbResult<Vec<Heartbeat>> {
    let before_unix = before.map(|t| t.unix_timestamp());
    let rows = sqlx::query(
        "SELECT monitor_id, ts, status, latency_ms, status_code, msg, retries, important
         FROM heartbeats
         WHERE monitor_id = ? AND (? IS NULL OR ts < ?)
         ORDER BY ts DESC LIMIT ?",
    )
    .bind(monitor.0.to_string())
    .bind(before_unix)
    .bind(before_unix)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(hb_from).collect())
}

pub async fn range_for_monitor(
    pool: &MySqlPool,
    monitor: MonitorId,
    since: OffsetDateTime,
    until: OffsetDateTime,
    limit: i64,
) -> DbResult<Vec<Heartbeat>> {
    let rows = sqlx::query(
        "SELECT monitor_id, ts, status, latency_ms, status_code, msg, retries, important
         FROM heartbeats
         WHERE monitor_id = ? AND ts >= ? AND ts < ?
         ORDER BY ts ASC LIMIT ?",
    )
    .bind(monitor.0.to_string())
    .bind(since.unix_timestamp())
    .bind(until.unix_timestamp())
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(hb_from).collect())
}

/// Uptime % over the trailing window. `None` on an empty window. The `SUM(CASE…)`
/// is cast to SIGNED so it decodes as i64 (MySQL SUM → DECIMAL otherwise).
pub async fn uptime_pct(
    pool: &MySqlPool,
    monitor: MonitorId,
    window_seconds: i64,
) -> DbResult<Option<f64>> {
    let (total, ok): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*),
                CAST(COALESCE(SUM(CASE WHEN status = 'up' THEN 1 ELSE 0 END), 0) AS SIGNED)
         FROM heartbeats WHERE monitor_id = ? AND ts >= ?",
    )
    .bind(monitor.0.to_string())
    .bind(since_secs(window_seconds))
    .fetch_one(pool)
    .await?;
    if total == 0 {
        return Ok(None);
    }
    Ok(Some(ok as f64 / total as f64 * 100.0))
}

/// Rolling SLO uptime over the trailing `window_days`, excluding maintenance.
pub async fn current_slo_uptime_pct(
    pool: &MySqlPool,
    monitor: MonitorId,
    window_days: i32,
) -> DbResult<Option<f64>> {
    let (total, ok): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*),
                CAST(COALESCE(SUM(CASE WHEN status = 'up' THEN 1 ELSE 0 END), 0) AS SIGNED)
         FROM heartbeats
         WHERE monitor_id = ? AND ts >= ? AND status <> 'maintenance'",
    )
    .bind(monitor.0.to_string())
    .bind(since_secs(window_days as i64 * 86_400))
    .fetch_one(pool)
    .await?;
    if total == 0 {
        return Ok(None);
    }
    Ok(Some(ok as f64 / total as f64 * 100.0))
}

/// Average latency (ms) over the trailing window for successful heartbeats.
pub async fn avg_latency_ms(
    pool: &MySqlPool,
    monitor: MonitorId,
    window_seconds: i64,
) -> DbResult<Option<f64>> {
    // `* 1e0` forces DOUBLE — MySQL AVG() of an INT column returns DECIMAL,
    // which won't decode into f64.
    let (avg,): (Option<f64>,) = sqlx::query_as(
        "SELECT AVG(latency_ms) * 1e0 FROM heartbeats
         WHERE monitor_id = ? AND ts >= ? AND status = 'up' AND latency_ms IS NOT NULL",
    )
    .bind(monitor.0.to_string())
    .bind(since_secs(window_seconds))
    .fetch_one(pool)
    .await?;
    Ok(avg)
}

/// Newest `per_monitor` heartbeats for every monitor in `org_id` (the dashboard
/// hero strip). ROW_NUMBER window; the derived table needs an alias on MySQL.
pub async fn recent_per_monitor(
    pool: &MySqlPool,
    per_monitor: i64,
    org_id: OrgId,
) -> DbResult<Vec<Heartbeat>> {
    let rows = sqlx::query(
        "SELECT monitor_id, ts, status, latency_ms, status_code, msg, retries, important FROM (
            SELECT h.monitor_id, h.ts, h.status, h.latency_ms, h.status_code, h.msg, h.retries,
                   h.important,
                   ROW_NUMBER() OVER (PARTITION BY h.monitor_id ORDER BY h.ts DESC) AS rn
            FROM heartbeats h JOIN monitors m ON m.id = h.monitor_id
            WHERE m.org_id = ?
         ) AS sub WHERE rn <= ? ORDER BY monitor_id, ts ASC",
    )
    .bind(org_id.0.to_string())
    .bind(per_monitor)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(hb_from).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mysql::monitors;
    use rampart_core::monitor::{MonitorKind, NewMonitor};
    use rampart_core::MonitorStatus;

    async fn monitor(pool: &MySqlPool) -> MonitorId {
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

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn insert_recent_uptime_avg(pool: MySqlPool) {
        let m = monitor(&pool).await;
        insert_many(
            &pool,
            &[
                hb(m, 40, MonitorStatus::Up),
                hb(m, 30, MonitorStatus::Up),
                hb(m, 20, MonitorStatus::Down),
                hb(m, 10, MonitorStatus::Up),
            ],
        )
        .await
        .unwrap();

        // newest-first history.
        let recent = recent_for_monitor(&pool, m, 10).await.unwrap();
        assert_eq!(recent.len(), 4);
        assert_eq!(recent[0].status, MonitorStatus::Up); // 10s ago (newest)
        assert_eq!(recent[1].status, MonitorStatus::Down); // 20s ago

        // 3 up / 4 total = 75% (the SUM→CAST AS SIGNED path).
        let up = uptime_pct(&pool, m, 3600).await.unwrap().unwrap();
        assert!((up - 75.0).abs() < 0.001, "uptime {up}");
        assert!(uptime_pct(&pool, m, 1).await.unwrap().is_none()); // empty window → None

        // SLO uptime (excludes maintenance — none here, same 75%).
        let slo = current_slo_uptime_pct(&pool, m, 30).await.unwrap().unwrap();
        assert!((slo - 75.0).abs() < 0.001);

        // avg latency over 'up' rows (all 42).
        let avg = avg_latency_ms(&pool, m, 3600).await.unwrap().unwrap();
        assert!((avg - 42.0).abs() < 0.001);

        // range + before paging.
        let now = OffsetDateTime::now_utc();
        let range = range_for_monitor(&pool, m, now - time::Duration::hours(1), now, 100)
            .await
            .unwrap();
        assert_eq!(range.len(), 4);
        assert!(range[0].ts <= range[3].ts); // ascending

        // recent_per_monitor caps per monitor.
        let org = super::super::oid("00000000-0000-0000-0000-000000000001");
        let per = recent_per_monitor(&pool, 2, org).await.unwrap();
        assert_eq!(per.len(), 2);
    }
}
