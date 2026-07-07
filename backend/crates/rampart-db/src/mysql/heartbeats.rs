//! MySQL `heartbeats` domain — the probe time-series writer, the history feeds,
//! the trailing-window uptime/latency reads, and the full dashboard analytics
//! (daily/monthly buckets, hourly latency, mtbf/mttr, SLO error budget +
//! burndown, the batch rollups, summary window).
//!
//! MySQL deltas vs SQLite: `ON CONFLICT DO NOTHING` → `ON DUPLICATE KEY`;
//! **`SUM(CASE…)` returns DECIMAL → `CAST(… AS SIGNED)` so it decodes as i64**;
//! **`AVG(int)` returns DECIMAL → `* 1e0` for f64**; `/` is decimal division →
//! `DIV` for integer day buckets; `strftime('%H'/'%Y-%m', ts, 'unixepoch')` →
//! `HOUR(FROM_UNIXTIME(ts))` / `DATE_FORMAT(FROM_UNIXTIME(ts), '%Y-%m')`; derived
//! tables need an alias (`) AS sub`). The MTBF/MTTR + error-budget walks reuse
//! PG's exact ascending-ts Rust logic (only the query is runtime-checked).
//! ts→BIGINT, bool→TINYINT, latency/status_code/retries i32→INT.

use super::{in_placeholders, mid, mstatus_from, mstatus_str, ts};
use crate::heartbeats::{BurndownPoint, ErrorBudget, MonitorSummary, MonthlyUptime, MtbfMttr};
use crate::prune::{DailyUptimePoint, HeartbeatRollup};
use crate::DbResult;
use rampart_core::ids::{MonitorId, OrgId};
use rampart_core::{Heartbeat, MonitorStatus};
use sqlx::{MySqlPool, Row};
use std::collections::{BTreeMap, HashMap};
use time::OffsetDateTime;
use uuid::Uuid;

fn since_secs(window_seconds: i64) -> i64 {
    OffsetDateTime::now_utc().unix_timestamp() - window_seconds
}

/// UTC calendar date for a whole-day bucket number (`ts DIV 86400`).
fn date_from_day_num(day_num: i64) -> time::Date {
    ts(day_num * 86_400).date()
}

/// First-of-month `Date` from a `DATE_FORMAT(…, '%Y-%m')` key ("2026-06").
fn month_first_from_key(ym: &str) -> Option<time::Date> {
    let (y, m) = ym.split_once('-')?;
    let year: i32 = y.parse().ok()?;
    let mon: u8 = m.parse().ok()?;
    time::Date::from_calendar_date(year, time::Month::try_from(mon).ok()?, 1).ok()
}

/// Dense oldest→newest month-first dates ending at the current month.
fn month_targets(months: i32) -> Vec<time::Date> {
    let now = OffsetDateTime::now_utc().date();
    let current = time::Date::from_calendar_date(now.year(), now.month(), 1).unwrap_or(now);
    let mut targets = Vec::with_capacity(months as usize);
    let mut y = current.year();
    let mut m_u8 = current.month() as u8;
    for _ in 0..months {
        let mth = time::Month::try_from(m_u8).unwrap_or(time::Month::January);
        targets.push(time::Date::from_calendar_date(y, mth, 1).unwrap_or(current));
        if m_u8 == 1 {
            m_u8 = 12;
            y -= 1;
        } else {
            m_u8 -= 1;
        }
    }
    targets.reverse();
    targets
}

fn status_char(any_down: bool, any_warn: bool, any_real: bool) -> u8 {
    if any_down {
        b'd'
    } else if any_warn {
        b'w'
    } else if !any_real {
        b'm'
    } else {
        b'u'
    }
}

/// Allowed-downtime budget for an SLO window: `(window_seconds, allowed_secs)`.
fn allowed_downtime(window_days: i32, target_pct: f64) -> (i64, i64) {
    let window_seconds = window_days as i64 * 86_400;
    let allowed = (((100.0 - target_pct) / 100.0) * window_seconds as f64)
        .round()
        .max(0.0) as i64;
    (window_seconds, allowed)
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
    for batch in hbs.chunks(super::insert_chunk(8)) {
        let mut qb = sqlx::QueryBuilder::new(
            "INSERT INTO heartbeats \
                (monitor_id, ts, status, latency_ms, status_code, msg, retries, important) ",
        );
        qb.push_values(batch, |mut b, h| {
            b.push_bind(h.monitor_id.0.to_string())
                .push_bind(h.ts.unix_timestamp())
                .push_bind(mstatus_str(h.status))
                .push_bind(h.latency_ms)
                .push_bind(h.status_code)
                .push_bind(&h.msg)
                .push_bind(h.retries)
                .push_bind(h.important as i64);
        });
        qb.push(" ON DUPLICATE KEY UPDATE monitor_id = monitor_id");
        qb.build().execute(&mut *tx).await?;
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
         FROM heartbeats WHERE monitor_id = ? AND ts >= ? AND status <> 'maintenance'",
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

/// Per-day status string (one byte/day, oldest→newest) for the uptime ribbon.
/// `date_trunc('day')::date` → `ts DIV 86400`; `BOOL_OR` → `MAX(CASE …)`.
pub async fn daily_status(pool: &MySqlPool, monitor: MonitorId, days: i32) -> DbResult<Vec<u8>> {
    let rows = sqlx::query(
        "SELECT ts DIV 86400 AS day_num,
                MAX(CASE WHEN status = 'down' THEN 1 ELSE 0 END) AS any_down,
                MAX(CASE WHEN status = 'warn' THEN 1 ELSE 0 END) AS any_warn,
                MAX(CASE WHEN status <> 'maintenance' THEN 1 ELSE 0 END) AS any_real
         FROM heartbeats WHERE monitor_id = ? AND ts >= ?
         GROUP BY day_num ORDER BY day_num",
    )
    .bind(monitor.0.to_string())
    .bind(since_secs(days as i64 * 86_400))
    .fetch_all(pool)
    .await?;
    let today = OffsetDateTime::now_utc().date();
    let mut out = vec![b'n'; days as usize];
    for r in &rows {
        let day = date_from_day_num(r.get::<i64, _>("day_num"));
        let delta = (today - day).whole_days();
        if delta < 0 || delta >= days as i64 {
            continue;
        }
        let idx = (days as i64 - 1 - delta) as usize;
        out[idx] = status_char(
            r.get::<i64, _>("any_down") != 0,
            r.get::<i64, _>("any_warn") != 0,
            r.get::<i64, _>("any_real") != 0,
        );
    }
    Ok(out)
}

/// Per-hour avg latency for one UTC calendar day (sparse). `EXTRACT(HOUR …)` →
/// `HOUR(FROM_UNIXTIME(ts))`; `AVG(int)` → `* 1e0` for f64.
pub async fn day_hourly_latency(
    pool: &MySqlPool,
    monitor: MonitorId,
    day: time::Date,
) -> DbResult<Vec<(i32, Option<f32>, i32)>> {
    let day_start = day
        .with_hms(0, 0, 0)
        .expect("00:00:00 is always valid")
        .assume_utc()
        .unix_timestamp();
    let day_end = day_start + 86_400;
    let rows = sqlx::query(
        "SELECT HOUR(FROM_UNIXTIME(ts)) AS hour,
                AVG(latency_ms) * 1e0 AS avg_latency_ms,
                COUNT(*) AS samples
         FROM heartbeats
         WHERE monitor_id = ? AND ts >= ? AND ts < ? AND status = 'up'
         GROUP BY hour ORDER BY hour",
    )
    .bind(monitor.0.to_string())
    .bind(day_start)
    .bind(day_end)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            (
                r.get::<i64, _>("hour") as i32,
                r.get::<Option<f64>, _>("avg_latency_ms").map(|v| v as f32),
                r.get::<i64, _>("samples") as i32,
            )
        })
        .collect())
}

/// Per-month uptime, dense, oldest first, today's month last.
pub async fn monthly_uptime(
    pool: &MySqlPool,
    monitor: MonitorId,
    months: i32,
) -> DbResult<Vec<MonthlyUptime>> {
    let rows = sqlx::query(
        "SELECT DATE_FORMAT(FROM_UNIXTIME(ts), '%Y-%m') AS ym,
                COUNT(*) AS total,
                CAST(COALESCE(SUM(CASE WHEN status = 'up' THEN 1 ELSE 0 END), 0) AS SIGNED) AS up
         FROM heartbeats WHERE monitor_id = ? AND ts >= ? AND status <> 'maintenance'
         GROUP BY ym ORDER BY ym",
    )
    .bind(monitor.0.to_string())
    .bind(since_secs(months as i64 * 32 * 86_400))
    .fetch_all(pool)
    .await?;
    let mut out: Vec<MonthlyUptime> = month_targets(months)
        .into_iter()
        .map(|d| MonthlyUptime {
            year_month: d,
            uptime_pct: None,
        })
        .collect();
    for r in &rows {
        let Some(month) = month_first_from_key(&r.get::<String, _>("ym")) else {
            continue;
        };
        let total = r.get::<i64, _>("total");
        if total > 0 {
            if let Some(slot) = out.iter_mut().find(|m| m.year_month == month) {
                slot.uptime_pct = Some(r.get::<i64, _>("up") as f32 / total as f32 * 100.0);
            }
        }
    }
    Ok(out)
}

/// Batch mirror of [`uptime_pct`] (absent → per-monitor `None`).
pub async fn uptime_pct_batch(
    pool: &MySqlPool,
    monitor_ids: &[Uuid],
    window_seconds: i64,
) -> DbResult<HashMap<Uuid, f64>> {
    if monitor_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let sql = format!(
        "SELECT monitor_id, COUNT(*) AS total,
                CAST(COALESCE(SUM(CASE WHEN status = 'up' THEN 1 ELSE 0 END), 0) AS SIGNED) AS ok_count
         FROM heartbeats WHERE monitor_id IN ({}) AND ts >= ? AND status <> 'maintenance'
         GROUP BY monitor_id",
        in_placeholders(monitor_ids.len())
    );
    let mut q = sqlx::query(sqlx::AssertSqlSafe(sql));
    for id in monitor_ids {
        q = q.bind(id.to_string());
    }
    let rows = q.bind(since_secs(window_seconds)).fetch_all(pool).await?;
    let mut out = HashMap::with_capacity(rows.len());
    for r in &rows {
        let total = r.get::<i64, _>("total");
        if total > 0 {
            out.insert(
                super::raw_uuid(&r.get::<String, _>("monitor_id")),
                r.get::<i64, _>("ok_count") as f64 / total as f64 * 100.0,
            );
        }
    }
    Ok(out)
}

/// Batch mirror of [`avg_latency_ms`]. `AVG(int)` → `* 1e0` for f64.
pub async fn avg_latency_ms_batch(
    pool: &MySqlPool,
    monitor_ids: &[Uuid],
    window_seconds: i64,
) -> DbResult<HashMap<Uuid, f64>> {
    if monitor_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let sql = format!(
        "SELECT monitor_id, AVG(latency_ms) * 1e0 AS avg
         FROM heartbeats
         WHERE monitor_id IN ({}) AND ts >= ? AND status = 'up' AND latency_ms IS NOT NULL
         GROUP BY monitor_id",
        in_placeholders(monitor_ids.len())
    );
    let mut q = sqlx::query(sqlx::AssertSqlSafe(sql));
    for id in monitor_ids {
        q = q.bind(id.to_string());
    }
    let rows = q.bind(since_secs(window_seconds)).fetch_all(pool).await?;
    let mut out = HashMap::with_capacity(rows.len());
    for r in &rows {
        if let Some(avg) = r.get::<Option<f64>, _>("avg") {
            out.insert(super::raw_uuid(&r.get::<String, _>("monitor_id")), avg);
        }
    }
    Ok(out)
}

/// Batch mirror of [`daily_status`] — every id present (empty → all-`n`).
pub async fn daily_status_batch(
    pool: &MySqlPool,
    monitor_ids: &[Uuid],
    days: i32,
) -> DbResult<HashMap<Uuid, Vec<u8>>> {
    let mut out: HashMap<Uuid, Vec<u8>> = monitor_ids
        .iter()
        .map(|id| (*id, vec![b'n'; days as usize]))
        .collect();
    if monitor_ids.is_empty() {
        return Ok(out);
    }
    let sql = format!(
        "SELECT monitor_id, ts DIV 86400 AS day_num,
                MAX(CASE WHEN status = 'down' THEN 1 ELSE 0 END) AS any_down,
                MAX(CASE WHEN status = 'warn' THEN 1 ELSE 0 END) AS any_warn,
                MAX(CASE WHEN status <> 'maintenance' THEN 1 ELSE 0 END) AS any_real
         FROM heartbeats WHERE monitor_id IN ({}) AND ts >= ?
         GROUP BY monitor_id, day_num",
        in_placeholders(monitor_ids.len())
    );
    let mut q = sqlx::query(sqlx::AssertSqlSafe(sql));
    for id in monitor_ids {
        q = q.bind(id.to_string());
    }
    let rows = q
        .bind(since_secs(days as i64 * 86_400))
        .fetch_all(pool)
        .await?;
    let today = OffsetDateTime::now_utc().date();
    for r in &rows {
        let day = date_from_day_num(r.get::<i64, _>("day_num"));
        let delta = (today - day).whole_days();
        if delta < 0 || delta >= days as i64 {
            continue;
        }
        let idx = (days as i64 - 1 - delta) as usize;
        if let Some(v) = out.get_mut(&super::raw_uuid(&r.get::<String, _>("monitor_id"))) {
            v[idx] = status_char(
                r.get::<i64, _>("any_down") != 0,
                r.get::<i64, _>("any_warn") != 0,
                r.get::<i64, _>("any_real") != 0,
            );
        }
    }
    Ok(out)
}

/// Batch mirror of [`monthly_uptime`] — every id present (empty → all-`None`).
pub async fn monthly_uptime_batch(
    pool: &MySqlPool,
    monitor_ids: &[Uuid],
    months: i32,
) -> DbResult<HashMap<Uuid, Vec<MonthlyUptime>>> {
    let template: Vec<MonthlyUptime> = month_targets(months)
        .into_iter()
        .map(|d| MonthlyUptime {
            year_month: d,
            uptime_pct: None,
        })
        .collect();
    let mut out: HashMap<Uuid, Vec<MonthlyUptime>> = monitor_ids
        .iter()
        .map(|id| (*id, template.clone()))
        .collect();
    if monitor_ids.is_empty() {
        return Ok(out);
    }
    let sql = format!(
        "SELECT monitor_id, DATE_FORMAT(FROM_UNIXTIME(ts), '%Y-%m') AS ym,
                COUNT(*) AS total,
                CAST(COALESCE(SUM(CASE WHEN status = 'up' THEN 1 ELSE 0 END), 0) AS SIGNED) AS up
         FROM heartbeats WHERE monitor_id IN ({}) AND ts >= ? AND status <> 'maintenance'
         GROUP BY monitor_id, ym",
        in_placeholders(monitor_ids.len())
    );
    let mut q = sqlx::query(sqlx::AssertSqlSafe(sql));
    for id in monitor_ids {
        q = q.bind(id.to_string());
    }
    let rows = q
        .bind(since_secs(months as i64 * 32 * 86_400))
        .fetch_all(pool)
        .await?;
    for r in &rows {
        let Some(month) = month_first_from_key(&r.get::<String, _>("ym")) else {
            continue;
        };
        let total = r.get::<i64, _>("total");
        if total == 0 {
            continue;
        }
        if let Some(v) = out.get_mut(&super::raw_uuid(&r.get::<String, _>("monitor_id"))) {
            if let Some(slot) = v.iter_mut().find(|m| m.year_month == month) {
                slot.uptime_pct = Some(r.get::<i64, _>("up") as f32 / total as f32 * 100.0);
            }
        }
    }
    Ok(out)
}

/// 24h-window rollup of every org-scoped monitor with heartbeats in the window.
/// `ARRAY_AGG(… ORDER BY)[1]` (latest status/ts) → a ROW_NUMBER window merged in
/// Rust; `FILTER` → `CASE`. The latest-row derived table needs an alias on MySQL.
pub async fn summary_window(
    pool: &MySqlPool,
    window_seconds: i64,
    org_id: OrgId,
) -> DbResult<Vec<MonitorSummary>> {
    let since = since_secs(window_seconds);
    let agg = sqlx::query(
        "SELECT h.monitor_id AS monitor_id,
                COUNT(*) AS total,
                CAST(COALESCE(SUM(CASE WHEN h.status = 'up' THEN 1 ELSE 0 END), 0) AS SIGNED) AS up,
                AVG(CASE WHEN h.status = 'up' THEN h.latency_ms ELSE NULL END) * 1e0 AS avg_latency_ms
         FROM heartbeats h JOIN monitors m ON m.id = h.monitor_id
         WHERE h.ts >= ? AND m.org_id = ?
         GROUP BY h.monitor_id",
    )
    .bind(since)
    .bind(org_id.0.to_string())
    .fetch_all(pool)
    .await?;
    let latest = sqlx::query(
        "SELECT monitor_id, status, ts FROM (
            SELECT h.monitor_id AS monitor_id, h.status AS status, h.ts AS ts,
                   ROW_NUMBER() OVER (PARTITION BY h.monitor_id ORDER BY h.ts DESC) AS rn
            FROM heartbeats h JOIN monitors m ON m.id = h.monitor_id
            WHERE h.ts >= ? AND m.org_id = ?
         ) AS sub WHERE rn = 1",
    )
    .bind(since)
    .bind(org_id.0.to_string())
    .fetch_all(pool)
    .await?;
    let mut last: HashMap<String, (MonitorStatus, OffsetDateTime)> = HashMap::new();
    for r in &latest {
        last.insert(
            r.get::<String, _>("monitor_id"),
            (
                mstatus_from(&r.get::<String, _>("status")),
                ts(r.get::<i64, _>("ts")),
            ),
        );
    }
    Ok(agg
        .iter()
        .map(|r| {
            let mid_s = r.get::<String, _>("monitor_id");
            let l = last.get(&mid_s);
            MonitorSummary {
                monitor_id: mid(&mid_s),
                total: r.get::<i64, _>("total"),
                up: r.get::<i64, _>("up"),
                avg_latency_ms: r.get::<Option<f64>, _>("avg_latency_ms"),
                last_status: l.map(|(s, _)| *s),
                last_ts: l.map(|(_, t)| *t),
            }
        })
        .collect())
}

/// (ts, status) ascending over a trailing window — the shared input for the
/// walk-based MTBF/MTTR + error-budget computations.
async fn walk_rows(
    pool: &MySqlPool,
    monitor: MonitorId,
    window_seconds: i64,
) -> DbResult<Vec<(OffsetDateTime, MonitorStatus)>> {
    let rows = sqlx::query(
        "SELECT ts, status FROM heartbeats
         WHERE monitor_id = ? AND ts >= ? ORDER BY ts ASC",
    )
    .bind(monitor.0.to_string())
    .bind(since_secs(window_seconds))
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            (
                ts(r.get::<i64, _>("ts")),
                mstatus_from(&r.get::<String, _>("status")),
            )
        })
        .collect())
}

/// MTBF/MTTR over a trailing window — identical timeline walk to PG.
pub async fn mtbf_mttr(
    pool: &MySqlPool,
    monitor: MonitorId,
    window_seconds: i64,
) -> DbResult<MtbfMttr> {
    let rows = walk_rows(pool, monitor, window_seconds).await?;
    if rows.is_empty() {
        return Ok(MtbfMttr {
            mtbf_secs: None,
            mttr_secs: None,
            downtime_events: 0,
        });
    }
    let (mut up_secs, mut down_secs, mut failures, mut recoveries) = (0i64, 0i64, 0i64, 0i64);
    for w in rows.windows(2) {
        let (pt, ps) = w[0];
        let (ct, cs) = w[1];
        let dur = (ct - pt).whole_seconds().max(0);
        match ps {
            MonitorStatus::Up => up_secs += dur,
            MonitorStatus::Down => down_secs += dur,
            _ => {}
        }
        match (ps, cs) {
            (MonitorStatus::Up, MonitorStatus::Down) => failures += 1,
            (MonitorStatus::Down, MonitorStatus::Up) => recoveries += 1,
            _ => {}
        }
    }
    Ok(MtbfMttr {
        mtbf_secs: (failures > 0).then(|| up_secs / failures),
        mttr_secs: (recoveries > 0).then(|| down_secs / recoveries),
        downtime_events: failures,
    })
}

/// SLO error-budget fuel gauge — identical down-segment walk to PG.
pub async fn error_budget(
    pool: &MySqlPool,
    monitor: MonitorId,
    window_days: i32,
    target_pct: f64,
) -> DbResult<ErrorBudget> {
    let (window_seconds, allowed) = allowed_downtime(window_days, target_pct);
    let rows = walk_rows(pool, monitor, window_seconds).await?;
    let mut used = 0i64;
    for w in rows.windows(2) {
        if matches!(w[0].1, MonitorStatus::Down) {
            used += (w[1].0 - w[0].0).whole_seconds().max(0);
        }
    }
    let remaining = (allowed - used).max(0);
    let remaining_pct = if allowed > 0 {
        (remaining as f64 / allowed as f64) * 100.0
    } else {
        100.0
    };
    Ok(ErrorBudget {
        window_days,
        target_pct,
        allowed_downtime_secs: allowed,
        used_downtime_secs: used,
        remaining_downtime_secs: remaining,
        remaining_pct,
    })
}

/// Day-by-day SLO error-budget burn-down, oldest first — identical to PG.
pub async fn error_budget_burndown(
    pool: &MySqlPool,
    monitor: MonitorId,
    window_days: i32,
    target_pct: f64,
) -> DbResult<Vec<BurndownPoint>> {
    let (window_seconds, allowed) = allowed_downtime(window_days, target_pct);
    let since = OffsetDateTime::now_utc() - time::Duration::seconds(window_seconds);
    let rows = walk_rows(pool, monitor, window_seconds).await?;
    let mut per_day: BTreeMap<time::Date, i64> = BTreeMap::new();
    for w in rows.windows(2) {
        if matches!(w[0].1, MonitorStatus::Down) {
            let dur = (w[1].0 - w[0].0).whole_seconds().max(0);
            *per_day
                .entry(w[0].0.to_offset(time::UtcOffset::UTC).date())
                .or_insert(0) += dur;
        }
    }
    let first_day = since.date();
    let today = OffsetDateTime::now_utc().date();
    let span = (today - first_day).whole_days().max(0);
    let mut out = Vec::with_capacity(span as usize + 1);
    let mut cumulative = 0i64;
    let mut day = first_day;
    for _ in 0..=span {
        cumulative += per_day.get(&day).copied().unwrap_or(0);
        let remaining = (allowed - cumulative).max(0);
        let remaining_pct = if allowed > 0 {
            ((remaining as f64 / allowed as f64) * 100.0).max(0.0)
        } else {
            100.0
        };
        out.push(BurndownPoint {
            day,
            cumulative_down_secs: cumulative,
            budget_remaining_secs: remaining,
            budget_remaining_pct: remaining_pct,
        });
        match day.next_day() {
            Some(d) => day = d,
            None => break,
        }
    }
    Ok(out)
}

/// Flat age-based retention prune: drop heartbeats older than `days`. The
/// Retention fold: aggregate raw heartbeats older than the raw tier into hourly
/// `heartbeat_rollups` buckets, delete those raw rows, then drop rollup buckets
/// past the (longer) `rollup_days` tier. Mirrors the Postgres/SQLite tiering so
/// long-range uptime history survives after the high-resolution rows are pruned.
/// Returns rows deleted (raw heartbeats + expired rollups).
///
/// Fold + raw delete run in ONE transaction, so a crash can't delete raw rows
/// that weren't durably rolled up first (retry re-folds cleanly on rollback; the
/// `ON DUPLICATE KEY UPDATE` accumulation is idempotent). NOTE (MySQL-specific):
/// the update assigns `avg_latency_ms` BEFORE `sample_count` because MySQL
/// evaluates `ON DUPLICATE KEY UPDATE` assignments left-to-right and later ones
/// see earlier updates — the weighted mean needs the PRE-update `sample_count`.
///
/// ponytail: folds the whole backlog in one statement (no batching). Fine for
/// the relational-subset tier; batch by `bucket_start` window if a deployment
/// ever accumulates millions of stale heartbeats between prune ticks.
pub async fn fold_and_prune(
    pool: &MySqlPool,
    heartbeat_days: i32,
    rollup_days: i32,
) -> DbResult<u64> {
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let cutoff = now - heartbeat_days.max(0) as i64 * 86_400;
    let rollup_cutoff = now - rollup_days.max(0) as i64 * 86_400;

    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO heartbeat_rollups
            (monitor_id, bucket_start, up_count, down_count, other_count, sample_count, avg_latency_ms)
         SELECT monitor_id,
                (ts DIV 3600) * 3600 AS bucket_start,
                SUM(status = 'up'),
                SUM(status = 'down'),
                SUM(status NOT IN ('up', 'down')),
                COUNT(*),
                AVG(latency_ms)
         FROM heartbeats
         WHERE ts < ?
         GROUP BY monitor_id, bucket_start
         ON DUPLICATE KEY UPDATE
            avg_latency_ms = CASE
                WHEN avg_latency_ms IS NULL THEN VALUES(avg_latency_ms)
                WHEN VALUES(avg_latency_ms) IS NULL THEN avg_latency_ms
                ELSE (avg_latency_ms * sample_count + VALUES(avg_latency_ms) * VALUES(sample_count))
                     / (sample_count + VALUES(sample_count))
            END,
            up_count     = up_count + VALUES(up_count),
            down_count   = down_count + VALUES(down_count),
            other_count  = other_count + VALUES(other_count),
            sample_count = sample_count + VALUES(sample_count)",
    )
    .bind(cutoff)
    .execute(&mut *tx)
    .await?;

    let deleted_raw = sqlx::query("DELETE FROM heartbeats WHERE ts < ?")
        .bind(cutoff)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    let deleted_rollups = sqlx::query("DELETE FROM heartbeat_rollups WHERE bucket_start < ?")
        .bind(rollup_cutoff)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    tx.commit().await?;
    Ok(deleted_raw + deleted_rollups)
}

/// Hourly rollups for a monitor over `[since, until)`, oldest first.
pub async fn rollups_for_monitor(
    pool: &MySqlPool,
    monitor: Uuid,
    since: OffsetDateTime,
    until: OffsetDateTime,
) -> DbResult<Vec<HeartbeatRollup>> {
    let rows = sqlx::query(
        "SELECT bucket_start, up_count, down_count, other_count, sample_count, avg_latency_ms
         FROM heartbeat_rollups
         WHERE monitor_id = ? AND bucket_start >= ? AND bucket_start < ?
         ORDER BY bucket_start ASC",
    )
    .bind(monitor.to_string())
    .bind(since.unix_timestamp())
    .bind(until.unix_timestamp())
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| HeartbeatRollup {
            monitor_id: monitor,
            bucket_start: ts(r.get::<i64, _>("bucket_start")),
            up_count: r.get::<i32, _>("up_count"),
            down_count: r.get::<i32, _>("down_count"),
            other_count: r.get::<i32, _>("other_count"),
            sample_count: r.get::<i32, _>("sample_count"),
            avg_latency_ms: r.get::<Option<f64>, _>("avg_latency_ms"),
        })
        .collect())
}

fn daily_point(day_num: i64, up: i64, samples: i64) -> DailyUptimePoint {
    DailyUptimePoint {
        day: date_from_day_num(day_num),
        up_count: up,
        sample_count: samples,
        uptime_pct: if samples > 0 {
            Some(up as f64 / samples as f64 * 100.0)
        } else {
            None
        },
    }
}

/// Daily uptime% from the hourly rollups over `[since, until)`, oldest first.
/// The long-range path (rollups outlive raw heartbeats).
pub async fn daily_uptime_from_rollups(
    pool: &MySqlPool,
    monitor: Uuid,
    since: OffsetDateTime,
    until: OffsetDateTime,
) -> DbResult<Vec<DailyUptimePoint>> {
    let rows = sqlx::query(
        "SELECT bucket_start DIV 86400 AS day_num,
                CAST(SUM(up_count) AS SIGNED)     AS up,
                CAST(SUM(sample_count) AS SIGNED) AS samples
         FROM heartbeat_rollups
         WHERE monitor_id = ? AND bucket_start >= ? AND bucket_start < ?
         GROUP BY day_num ORDER BY day_num ASC",
    )
    .bind(monitor.to_string())
    .bind(since.unix_timestamp())
    .bind(until.unix_timestamp())
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            daily_point(
                r.get::<i64, _>("day_num"),
                r.get::<i64, _>("up"),
                r.get::<i64, _>("samples"),
            )
        })
        .collect())
}

/// Daily uptime% from the raw heartbeats over `[since, until)`, oldest first —
/// the recent within-retention portion, stitched onto the rollup series.
pub async fn daily_uptime_from_raw(
    pool: &MySqlPool,
    monitor: Uuid,
    since: OffsetDateTime,
    until: OffsetDateTime,
) -> DbResult<Vec<DailyUptimePoint>> {
    let rows = sqlx::query(
        "SELECT ts DIV 86400 AS day_num,
                CAST(SUM(status = 'up') AS SIGNED) AS up,
                CAST(COUNT(*) AS SIGNED)           AS samples
         FROM heartbeats
         WHERE monitor_id = ? AND ts >= ? AND ts < ?
         GROUP BY day_num ORDER BY day_num ASC",
    )
    .bind(monitor.to_string())
    .bind(since.unix_timestamp())
    .bind(until.unix_timestamp())
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            daily_point(
                r.get::<i64, _>("day_num"),
                r.get::<i64, _>("up"),
                r.get::<i64, _>("samples"),
            )
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mysql::monitors;
    use rampart_core::monitor::{MonitorKind, NewMonitor};

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
    async fn fold_and_prune_rolls_up_old_and_keeps_recent(pool: MySqlPool) {
        let m = monitor(&pool).await;
        insert_many(
            &pool,
            &[
                hb(m, 2 * 86400, MonitorStatus::Up), // 2 days old  → folded + deleted
                hb(m, 2 * 86400 + 60, MonitorStatus::Down), // same hour, other status
                hb(m, 10, MonitorStatus::Up),        // within retention → kept raw
            ],
        )
        .await
        .unwrap();

        // raw tier = 1 day, rollup tier = 365 days.
        let deleted = fold_and_prune(&pool, 1, 365).await.unwrap();
        assert_eq!(deleted, 2, "the two 2-day-old beats are folded + deleted");
        let left = recent_for_monitor(&pool, m, 10).await.unwrap();
        assert_eq!(left.len(), 1, "the recent beat survives as raw");

        let since = ts(OffsetDateTime::now_utc().unix_timestamp() - 30 * 86400);
        let until = OffsetDateTime::now_utc();

        // The old day survives in the rollup tier: 2 samples, 1 up.
        let roll = daily_uptime_from_rollups(&pool, m.0, since, until)
            .await
            .unwrap();
        assert_eq!(roll.iter().map(|d| d.sample_count).sum::<i64>(), 2);
        assert_eq!(roll.iter().map(|d| d.up_count).sum::<i64>(), 1);

        // The recent beat is still countable from the raw tier.
        let raw = daily_uptime_from_raw(&pool, m.0, since, until)
            .await
            .unwrap();
        assert_eq!(raw.iter().map(|d| d.sample_count).sum::<i64>(), 1);
        assert_eq!(raw.iter().map(|d| d.up_count).sum::<i64>(), 1);

        // rollups_for_monitor returns the hourly bucket(s).
        let buckets = rollups_for_monitor(&pool, m.0, since, until).await.unwrap();
        assert_eq!(buckets.iter().map(|b| b.sample_count).sum::<i32>(), 2);
        assert_eq!(buckets.iter().map(|b| b.up_count).sum::<i32>(), 1);
        assert_eq!(buckets.iter().map(|b| b.down_count).sum::<i32>(), 1);
    }

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn fold_is_idempotent_across_runs(pool: MySqlPool) {
        let m = monitor(&pool).await;
        insert_many(&pool, &[hb(m, 3 * 86400, MonitorStatus::Up)])
            .await
            .unwrap();
        fold_and_prune(&pool, 1, 365).await.unwrap();
        // A second run with nothing new to fold must not double-count.
        fold_and_prune(&pool, 1, 365).await.unwrap();
        let since = ts(OffsetDateTime::now_utc().unix_timestamp() - 30 * 86400);
        let roll = daily_uptime_from_rollups(&pool, m.0, since, OffsetDateTime::now_utc())
            .await
            .unwrap();
        assert_eq!(roll.iter().map(|d| d.sample_count).sum::<i64>(), 1);
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

    /// Exercises every analytics path with their MySQL-specific dialect (DIV
    /// buckets, DATE_FORMAT/HOUR, SUM→CAST SIGNED, AVG→*1e0, ROW_NUMBER) plus
    /// the Rust walk-based mtbf/error-budget over a Up→Down→Up timeline.
    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn analytics_rollups(pool: MySqlPool) {
        let m = monitor(&pool).await;
        let org = super::super::oid("00000000-0000-0000-0000-000000000001");
        // Up@300s ago → Down@200s ago → Up@100s ago: one failure + one recovery,
        // 100s up-segment then 100s down-segment.
        insert_many(
            &pool,
            &[
                hb(m, 300, MonitorStatus::Up),
                hb(m, 200, MonitorStatus::Down),
                hb(m, 100, MonitorStatus::Up),
            ],
        )
        .await
        .unwrap();

        // daily_status: today has a down sample → 'd'.
        assert_eq!(daily_status(&pool, m, 1).await.unwrap(), vec![b'd']);

        // monthly_uptime: 2 up / 3 total ≈ 66.7% in the latest (current) month.
        let mu = monthly_uptime(&pool, m, 1).await.unwrap();
        let pct = mu.last().unwrap().uptime_pct.unwrap();
        assert!((pct - 66.666_67).abs() < 0.1, "monthly {pct}");

        // mtbf/mttr: 100s up before the one failure, 100s down before recovery.
        let mm = mtbf_mttr(&pool, m, 3600).await.unwrap();
        assert_eq!(mm.downtime_events, 1);
        assert_eq!(mm.mtbf_secs, Some(100));
        assert_eq!(mm.mttr_secs, Some(100));

        // error_budget over 1 day @ 99%: allowed 864s, used 100s.
        let eb = error_budget(&pool, m, 1, 99.0).await.unwrap();
        assert_eq!(eb.allowed_downtime_secs, 864);
        assert_eq!(eb.used_downtime_secs, 100);
        assert_eq!(eb.remaining_downtime_secs, 764);

        // burndown last point carries the full cumulative downtime.
        let bd = error_budget_burndown(&pool, m, 1, 99.0).await.unwrap();
        assert_eq!(bd.last().unwrap().cumulative_down_secs, 100);

        // summary_window: 3 total, 2 up, newest status Up.
        let sw = summary_window(&pool, 3600, org).await.unwrap();
        let s = sw.iter().find(|s| s.monitor_id == m).unwrap();
        assert_eq!(s.total, 3);
        assert_eq!(s.up, 2);
        assert_eq!(s.last_status, Some(MonitorStatus::Up));

        // batch rollups (Uuid-keyed).
        let up_b = uptime_pct_batch(&pool, &[m.0], 3600).await.unwrap();
        assert!((up_b[&m.0] - 66.666_67).abs() < 0.1);
        let av_b = avg_latency_ms_batch(&pool, &[m.0], 3600).await.unwrap();
        assert!((av_b[&m.0] - 42.0).abs() < 0.001);
        assert_eq!(
            daily_status_batch(&pool, &[m.0], 1).await.unwrap()[&m.0],
            vec![b'd']
        );
        let mo_b = monthly_uptime_batch(&pool, &[m.0], 1).await.unwrap();
        assert!(mo_b[&m.0].last().unwrap().uptime_pct.unwrap() > 60.0);

        // day_hourly_latency: any populated hour reflects the up-row latency.
        let hl = day_hourly_latency(&pool, m, OffsetDateTime::now_utc().date())
            .await
            .unwrap();
        if let Some((_, Some(avg), _)) = hl.first() {
            assert!((avg - 42.0).abs() < 0.001, "hourly avg {avg}");
        }
    }
}
