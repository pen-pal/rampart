//! SQLite `heartbeats` domain — the probe time-series. Mirrors the Postgres
//! `crate::heartbeats` surface: the writer (`insert_many`), the history feeds
//! (`recent_for_monitor` / `recent_for_monitor_before` / `range_for_monitor` /
//! `recent_per_monitor`), and the full analytic set —
//! `uptime_pct` / `current_slo_uptime_pct` / `avg_latency_ms` / `daily_status` /
//! `day_hourly_latency` / `monthly_uptime` / `summary_window` / `mtbf_mttr` /
//! `error_budget` / `error_budget_burndown` plus the batch rollups.
//!
//! PG-ism → SQLite translation: `COUNT(*) FILTER (WHERE …)` → `SUM(CASE …)`;
//! `BOOL_OR` → `MAX(CASE …)`; `date_trunc('day', …)::date` → the whole-day
//! bucket `ts / 86400` (then `date_from_day_num`); `date_trunc('month', …)` /
//! `EXTRACT(HOUR …)` → `strftime('%Y-%m' / '%H', ts, 'unixepoch')`;
//! `ARRAY_AGG(… ORDER BY)[1]` (latest) → a `ROW_NUMBER()` window merged in Rust;
//! `= ANY($1)` → a bound `IN (?,…)` list. The MTBF/MTTR + error-budget walks
//! reuse PG's exact ascending-ts Rust logic (only the query is runtime-checked).
//!
//! No UNNEST on SQLite, so `insert_many` loops per-row inside one transaction.
//! `ts` is INTEGER unix-seconds; status is TEXT; `important` is INTEGER 0/1.

use super::{in_placeholders, mid, mstatus_from, mstatus_str, ts};
use crate::heartbeats::{BurndownPoint, ErrorBudget, MonitorSummary, MonthlyUptime, MtbfMttr};
use crate::DbResult;
use rampart_core::ids::{MonitorId, OrgId};
use rampart_core::{Heartbeat, MonitorStatus};
use sqlx::{Row, SqlitePool};
use std::collections::{BTreeMap, HashMap};
use time::OffsetDateTime;
use uuid::Uuid;

/// Trailing-window cutoff as unix-seconds: `now - window_seconds`.
fn since_secs(window_seconds: i64) -> i64 {
    OffsetDateTime::now_utc().unix_timestamp() - window_seconds
}

/// UTC calendar date for a whole-day bucket number (`ts / 86400`).
fn date_from_day_num(day_num: i64) -> time::Date {
    ts(day_num * 86_400).date()
}

/// First-of-month `Date` from a `strftime('%Y-%m')` key ("2026-06").
fn month_first_from_key(ym: &str) -> Option<time::Date> {
    let (y, m) = ym.split_once('-')?;
    let year: i32 = y.parse().ok()?;
    let mon: u8 = m.parse().ok()?;
    time::Date::from_calendar_date(year, time::Month::try_from(mon).ok()?, 1).ok()
}

/// The dense oldest→newest list of month-first dates ending at the current
/// month — shared by `monthly_uptime` and its batch twin (mirrors PG).
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

/// Paginated history — heartbeats strictly older than `before` (when set),
/// newest first. Mirrors PG `recent_for_monitor_before`.
pub async fn recent_for_monitor_before(
    pool: &SqlitePool,
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

/// All heartbeats in `[since, until)` oldest→newest (CSV export; caller caps).
pub async fn range_for_monitor(
    pool: &SqlitePool,
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

/// Rolling SLO uptime over the trailing `window_days`, excluding maintenance
/// from both numerator and denominator. `None` when the window is empty.
pub async fn current_slo_uptime_pct(
    pool: &SqlitePool,
    monitor: MonitorId,
    window_days: i32,
) -> DbResult<Option<f64>> {
    let (total, ok): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(CASE WHEN status = 'up' THEN 1 ELSE 0 END), 0)
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
    pool: &SqlitePool,
    monitor: MonitorId,
    window_seconds: i64,
) -> DbResult<Option<f64>> {
    let (avg,): (Option<f64>,) = sqlx::query_as(
        "SELECT AVG(latency_ms) FROM heartbeats
         WHERE monitor_id = ? AND ts >= ? AND status = 'up' AND latency_ms IS NOT NULL",
    )
    .bind(monitor.0.to_string())
    .bind(since_secs(window_seconds))
    .fetch_one(pool)
    .await?;
    Ok(avg)
}

/// Per-day status chars (`u`/`d`/`w`/`m`/`n`), dense, oldest first, today last.
/// `date_trunc('day', ts)::date` → group by the whole-day bucket `ts/86400`;
/// `BOOL_OR` → `MAX(CASE …)`. Pivot logic mirrors PG.
pub async fn daily_status(pool: &SqlitePool, monitor: MonitorId, days: i32) -> DbResult<Vec<u8>> {
    let rows = sqlx::query(
        "SELECT ts / 86400 AS day_num,
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

/// Per-hour avg latency for one UTC calendar day (sparse). `EXTRACT(HOUR …)` →
/// `strftime('%H', ts, 'unixepoch')`.
pub async fn day_hourly_latency(
    pool: &SqlitePool,
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
        "SELECT CAST(strftime('%H', ts, 'unixepoch') AS INTEGER) AS hour,
                AVG(latency_ms) AS avg_latency_ms,
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
    pool: &SqlitePool,
    monitor: MonitorId,
    months: i32,
) -> DbResult<Vec<MonthlyUptime>> {
    let rows = sqlx::query(
        "SELECT strftime('%Y-%m', ts, 'unixepoch') AS ym,
                COUNT(*) AS total,
                SUM(CASE WHEN status = 'up' THEN 1 ELSE 0 END) AS up
         FROM heartbeats WHERE monitor_id = ? AND ts >= ?
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
    pool: &SqlitePool,
    monitor_ids: &[Uuid],
    window_seconds: i64,
) -> DbResult<HashMap<Uuid, f64>> {
    if monitor_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let sql = format!(
        "SELECT monitor_id, COUNT(*) AS total,
                SUM(CASE WHEN status = 'up' THEN 1 ELSE 0 END) AS ok_count
         FROM heartbeats WHERE monitor_id IN ({}) AND ts >= ?
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

/// Batch mirror of [`avg_latency_ms`].
pub async fn avg_latency_ms_batch(
    pool: &SqlitePool,
    monitor_ids: &[Uuid],
    window_seconds: i64,
) -> DbResult<HashMap<Uuid, f64>> {
    if monitor_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let sql = format!(
        "SELECT monitor_id, AVG(latency_ms) AS avg
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
    pool: &SqlitePool,
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
        "SELECT monitor_id, ts / 86400 AS day_num,
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
    pool: &SqlitePool,
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
        "SELECT monitor_id, strftime('%Y-%m', ts, 'unixepoch') AS ym,
                COUNT(*) AS total,
                SUM(CASE WHEN status = 'up' THEN 1 ELSE 0 END) AS up
         FROM heartbeats WHERE monitor_id IN ({}) AND ts >= ?
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
/// `ARRAY_AGG(... ORDER BY)[1]` (latest status/ts) → a second windowed query
/// merged in Rust; `FILTER` → `CASE`.
pub async fn summary_window(
    pool: &SqlitePool,
    window_seconds: i64,
    org_id: OrgId,
) -> DbResult<Vec<MonitorSummary>> {
    let since = since_secs(window_seconds);
    let agg = sqlx::query(
        "SELECT h.monitor_id AS monitor_id,
                COUNT(*) AS total,
                SUM(CASE WHEN h.status = 'up' THEN 1 ELSE 0 END) AS up,
                AVG(CASE WHEN h.status = 'up' THEN h.latency_ms ELSE NULL END) AS avg_latency_ms
         FROM heartbeats h JOIN monitors m ON m.id = h.monitor_id
         WHERE h.ts >= ? AND m.org_id = ?
         GROUP BY h.monitor_id",
    )
    .bind(since)
    .bind(org_id.0.to_string())
    .fetch_all(pool)
    .await?;
    // Latest (status, ts) per monitor via ROW_NUMBER window.
    let latest = sqlx::query(
        "SELECT monitor_id, status, ts FROM (
            SELECT h.monitor_id AS monitor_id, h.status AS status, h.ts AS ts,
                   ROW_NUMBER() OVER (PARTITION BY h.monitor_id ORDER BY h.ts DESC) AS rn
            FROM heartbeats h JOIN monitors m ON m.id = h.monitor_id
            WHERE h.ts >= ? AND m.org_id = ?
         ) WHERE rn = 1",
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

/// Read (ts, status) ascending for a trailing window — the shared input for the
/// walk-based MTBF/MTTR + error-budget computations.
async fn walk_rows(
    pool: &SqlitePool,
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
    pool: &SqlitePool,
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

/// Compute the allowed-downtime budget for an SLO window (mirrors PG).
fn allowed_downtime(window_days: i32, target_pct: f64) -> (i64, i64) {
    let window_seconds = window_days as i64 * 86_400;
    let allowed = (((100.0 - target_pct) / 100.0) * window_seconds as f64)
        .round()
        .max(0.0) as i64;
    (window_seconds, allowed)
}

/// SLO error-budget fuel gauge — identical down-segment walk to PG.
pub async fn error_budget(
    pool: &SqlitePool,
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
    pool: &SqlitePool,
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

/// Last `per_monitor` heartbeats for every org-scoped monitor, oldest-first
/// within each. ROW_NUMBER window (SQLite 3.25+).
pub async fn recent_per_monitor(
    pool: &SqlitePool,
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
         ) WHERE rn <= ? ORDER BY monitor_id, ts ASC",
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

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn analytics_walks_and_aggregates(pool: SqlitePool) {
        let org = super::super::oid("00000000-0000-0000-0000-000000000001");
        let m = monitor(&pool).await;
        // Timeline (ASC): Up@400 → Down@300 → Down@200 → Up@100.
        // Segments: Up 100s (400→300), Down 100s (300→200), Down 100s (200→100).
        // ⇒ failures=1, recoveries=1, up=100, down=200.
        insert_many(
            &pool,
            &[
                hb(m, 400, MonitorStatus::Up),
                hb(m, 300, MonitorStatus::Down),
                hb(m, 200, MonitorStatus::Down),
                hb(m, 100, MonitorStatus::Up),
            ],
        )
        .await
        .unwrap();

        let mm = mtbf_mttr(&pool, m, 3600).await.unwrap();
        assert_eq!(mm.downtime_events, 1);
        assert_eq!(mm.mtbf_secs, Some(100));
        assert_eq!(mm.mttr_secs, Some(200));

        // error_budget: window 1d, target 99% → allowed 864s; used 200s.
        let eb = error_budget(&pool, m, 1, 99.0).await.unwrap();
        assert_eq!(eb.allowed_downtime_secs, 864);
        assert_eq!(eb.used_downtime_secs, 200);
        assert_eq!(eb.remaining_downtime_secs, 664);
        // burndown ends at the same cumulative used.
        let bd = error_budget_burndown(&pool, m, 1, 99.0).await.unwrap();
        assert_eq!(bd.last().unwrap().cumulative_down_secs, 200);

        // avg latency (up-only) = 42; SLO uptime = 2 up / 4 = 50%.
        assert_eq!(avg_latency_ms(&pool, m, 3600).await.unwrap(), Some(42.0));
        let slo = current_slo_uptime_pct(&pool, m, 1).await.unwrap().unwrap();
        assert!((slo - 50.0).abs() < 0.001, "slo {slo}");

        // daily_status: all today, has a down → today's bucket is 'd'.
        let ds = daily_status(&pool, m, 1).await.unwrap();
        assert_eq!(ds.len(), 1);
        assert_eq!(ds[0], b'd');
        assert_eq!(monthly_uptime(&pool, m, 1).await.unwrap().len(), 1);

        // summary_window: 4 total, 2 up, newest is Up, avg 42.
        let sw = summary_window(&pool, 3600, org).await.unwrap();
        assert_eq!(sw.len(), 1);
        assert_eq!(sw[0].total, 4);
        assert_eq!(sw[0].up, 2);
        assert_eq!(sw[0].last_status, Some(MonitorStatus::Up));
        assert_eq!(sw[0].avg_latency_ms, Some(42.0));

        // batch mirrors + recent_per_monitor + pagination.
        let ub = uptime_pct_batch(&pool, &[m.0], 3600).await.unwrap();
        assert!((ub.get(&m.0).unwrap() - 50.0).abs() < 0.001);
        assert_eq!(
            daily_status_batch(&pool, &[m.0], 1)
                .await
                .unwrap()
                .get(&m.0)
                .unwrap()[0],
            b'd'
        );
        assert_eq!(recent_per_monitor(&pool, 2, org).await.unwrap().len(), 2);
        // recent_for_monitor_before(now-150s) → the three older than 150s (400,300,200).
        let cutoff =
            OffsetDateTime::from_unix_timestamp(OffsetDateTime::now_utc().unix_timestamp() - 150)
                .unwrap();
        assert_eq!(
            recent_for_monitor_before(&pool, m, 10, Some(cutoff))
                .await
                .unwrap()
                .len(),
            3
        );
    }
}
