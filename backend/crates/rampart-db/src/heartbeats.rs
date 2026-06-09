//! Heartbeat queries.
//!
//! Renamed from `checks.rs`. Inserts are bulk-batched by the scheduler;
//! reads are by monitor + recent time window for the dashboard.

use crate::{DbPool, DbResult};
use rampart_core::{Heartbeat, MonitorId, MonitorStatus};
use time::OffsetDateTime;
use uuid::Uuid;

pub async fn insert_many(pool: &DbPool, hbs: &[Heartbeat]) -> DbResult<()> {
    if hbs.is_empty() {
        return Ok(());
    }

    // UNNEST-driven bulk insert. The `monitor_status` enum doesn't
    // derive `PgHasArrayType` for free, so we bind `statuses` as
    // text[] and cast in SQL via `::monitor_status[]`.
    let mut monitor_ids: Vec<Uuid> = Vec::with_capacity(hbs.len());
    let mut tss: Vec<OffsetDateTime> = Vec::with_capacity(hbs.len());
    let mut statuses: Vec<String> = Vec::with_capacity(hbs.len());
    let mut latencies: Vec<Option<i32>> = Vec::with_capacity(hbs.len());
    let mut codes: Vec<Option<i32>> = Vec::with_capacity(hbs.len());
    let mut msgs: Vec<Option<String>> = Vec::with_capacity(hbs.len());
    let mut retries: Vec<i32> = Vec::with_capacity(hbs.len());
    let mut importants: Vec<bool> = Vec::with_capacity(hbs.len());

    for h in hbs {
        monitor_ids.push(h.monitor_id.0);
        tss.push(h.ts);
        statuses.push(status_str(h.status).to_string());
        latencies.push(h.latency_ms);
        codes.push(h.status_code);
        msgs.push(h.msg.clone());
        retries.push(h.retries);
        importants.push(h.important);
    }

    sqlx::query!(
        r#"
        INSERT INTO heartbeats
            (monitor_id, ts, status, latency_ms, status_code, msg, retries, important)
        SELECT * FROM UNNEST(
            $1::uuid[],
            $2::timestamptz[],
            $3::text[]::monitor_status[],
            $4::int[],
            $5::int[],
            $6::text[],
            $7::int[],
            $8::bool[]
        )
        ON CONFLICT (monitor_id, ts) DO NOTHING
        "#,
        &monitor_ids[..],
        &tss[..],
        &statuses[..],
        &latencies[..] as &[Option<i32>],
        &codes[..] as &[Option<i32>],
        &msgs[..] as &[Option<String>],
        &retries[..],
        &importants[..],
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn recent_for_monitor(
    pool: &DbPool,
    monitor: MonitorId,
    limit: i64,
) -> DbResult<Vec<Heartbeat>> {
    recent_for_monitor_before(pool, monitor, limit, None).await
}

/// Paginated variant — pass `before` to fetch heartbeats strictly older
/// than that timestamp, in descending-ts order. The "Load more" UI sets
/// this to the oldest already-loaded heartbeat's ts.
pub async fn recent_for_monitor_before(
    pool: &DbPool,
    monitor: MonitorId,
    limit: i64,
    before: Option<time::OffsetDateTime>,
) -> DbResult<Vec<Heartbeat>> {
    let rows = sqlx::query!(
        r#"
        SELECT
            monitor_id,
            ts,
            status AS "status: MonitorStatus",
            latency_ms,
            status_code,
            msg,
            retries,
            important
        FROM heartbeats
        WHERE monitor_id = $1
          AND ($3::timestamptz IS NULL OR ts < $3)
        ORDER BY ts DESC
        LIMIT $2
        "#,
        monitor.0,
        limit,
        before,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| Heartbeat {
            monitor_id: MonitorId::from_uuid(r.monitor_id),
            ts: r.ts,
            status: r.status,
            latency_ms: r.latency_ms,
            status_code: r.status_code,
            msg: r.msg,
            retries: r.retries,
            important: r.important,
        })
        .collect())
}

/// All heartbeats for a monitor in `[since, until)` ordered oldest →
/// newest. Used by the CSV export endpoint — limit is enforced at the
/// API layer (we don't want a sprawling SELECT here).
pub async fn range_for_monitor(
    pool: &DbPool,
    monitor: MonitorId,
    since: time::OffsetDateTime,
    until: time::OffsetDateTime,
    limit: i64,
) -> DbResult<Vec<Heartbeat>> {
    let rows = sqlx::query!(
        r#"
        SELECT
            monitor_id,
            ts,
            status AS "status: MonitorStatus",
            latency_ms,
            status_code,
            msg,
            retries,
            important
        FROM heartbeats
        WHERE monitor_id = $1
          AND ts >= $2
          AND ts <  $3
        ORDER BY ts ASC
        LIMIT $4
        "#,
        monitor.0,
        since,
        until,
        limit,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| Heartbeat {
            monitor_id: MonitorId::from_uuid(r.monitor_id),
            ts: r.ts,
            status: r.status,
            latency_ms: r.latency_ms,
            status_code: r.status_code,
            msg: r.msg,
            retries: r.retries,
            important: r.important,
        })
        .collect())
}

/// Uptime % over the trailing `window_seconds`. Returns None if no
/// heartbeats recorded in the window (caller decides how to render).
pub async fn uptime_pct(
    pool: &DbPool,
    monitor: MonitorId,
    window_seconds: i64,
) -> DbResult<Option<f64>> {
    let since = OffsetDateTime::now_utc() - time::Duration::seconds(window_seconds);
    let row = sqlx::query!(
        r#"
        SELECT
            COUNT(*)                              AS total,
            COUNT(*) FILTER (WHERE status = 'up') AS ok_count
        FROM heartbeats
        WHERE monitor_id = $1 AND ts >= $2
        "#,
        monitor.0,
        since,
    )
    .fetch_one(pool)
    .await?;

    let total = row.total.unwrap_or(0);
    if total == 0 {
        return Ok(None);
    }
    let ok = row.ok_count.unwrap_or(0);
    Ok(Some(ok as f64 / total as f64 * 100.0))
}

/// Average latency in milliseconds over the trailing window for
/// successful heartbeats only. Used for the dashboard's p50 column.
pub async fn avg_latency_ms(
    pool: &DbPool,
    monitor: MonitorId,
    window_seconds: i64,
) -> DbResult<Option<f64>> {
    let since = OffsetDateTime::now_utc() - time::Duration::seconds(window_seconds);
    let row = sqlx::query!(
        r#"
        SELECT AVG(latency_ms)::float8 AS avg
        FROM heartbeats
        WHERE monitor_id = $1 AND ts >= $2 AND status = 'up' AND latency_ms IS NOT NULL
        "#,
        monitor.0,
        since,
    )
    .fetch_one(pool)
    .await?;
    Ok(row.avg)
}

/// Per-day uptime buckets for the trailing N days. Returns a vector of
/// length `days`, oldest first. Each entry is `(date_offset_days_ago,
/// status_char)` where status_char is:
///   'u' — at least one heartbeat that day, all were up
///   'd' — at least one heartbeat that day, any was down
///   'w' — at least one heartbeat that day, any was warn (and none down)
///   'm' — only maintenance heartbeats that day
///   'n' — no heartbeats at all that day (no data)
///
/// Powers the per-monitor 90-day timeline strip on public status pages.
/// Single query with GROUP BY date_trunc — cheap enough to call once
/// per monitor on every public-view scrape.
pub async fn daily_status(pool: &DbPool, monitor: MonitorId, days: i32) -> DbResult<Vec<u8>> {
    let since = OffsetDateTime::now_utc() - time::Duration::days(days as i64);
    let rows = sqlx::query!(
        r#"
        SELECT
            (date_trunc('day', ts AT TIME ZONE 'UTC'))::date AS "day!",
            BOOL_OR(status = 'down')        AS "any_down!",
            BOOL_OR(status = 'warn')        AS "any_warn!",
            BOOL_OR(status != 'maintenance') AS "any_real!"
        FROM heartbeats
        WHERE monitor_id = $1 AND ts >= $2
        GROUP BY 1
        ORDER BY 1
        "#,
        monitor.0,
        since,
    )
    .fetch_all(pool)
    .await?;

    // Pivot the sparse query result into a dense `days`-length vector,
    // oldest day first. Today's bucket is the final element.
    let today = OffsetDateTime::now_utc().date();
    let mut out = vec![b'n'; days as usize];
    for r in rows {
        let delta = (today - r.day).whole_days();
        if delta < 0 || delta >= days as i64 {
            continue;
        }
        let idx = (days as i64 - 1 - delta) as usize;
        out[idx] = if r.any_down {
            b'd'
        } else if r.any_warn {
            b'w'
        } else if !r.any_real {
            b'm'
        } else {
            b'u'
        };
    }
    Ok(out)
}

fn status_str(s: MonitorStatus) -> &'static str {
    match s {
        MonitorStatus::Up => "up",
        MonitorStatus::Down => "down",
        MonitorStatus::Warn => "warn",
        MonitorStatus::Paused => "paused",
        MonitorStatus::Pending => "pending",
        MonitorStatus::Maintenance => "maintenance",
    }
}

/// One monthly uptime bucket — the chips rendered under the daily
/// strip on the public status page. `year_month` is the first day of
/// the month so the UI can format it however it wants; `uptime_pct`
/// is null when no heartbeats were recorded that month.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MonthlyUptime {
    pub year_month: time::Date,
    pub uptime_pct: Option<f32>,
}

/// Per-month uptime for the trailing `months` months, oldest first.
/// Today's month is the final element; a month with zero heartbeats
/// recorded gets `uptime_pct = None` so the UI can render a "no data"
/// chip instead of a misleading 0%.
///
/// Single GROUP BY query + a dense-pivot pass mirroring `daily_status`.
/// Powers the "Jun 99.97% · Jul 100% · …" summary row every modern
/// status page (Stripe, GitHub, Cloudflare, Anthropic) ships under
/// the daily strip.
pub async fn monthly_uptime(
    pool: &DbPool,
    monitor: MonitorId,
    months: i32,
) -> DbResult<Vec<MonthlyUptime>> {
    // Look back `months` calendar months. Over-collect by a few days
    // (`months * 32`) so a January 31 -> February 28 boundary doesn't
    // accidentally drop January's bucket.
    let since = OffsetDateTime::now_utc() - time::Duration::days((months as i64) * 32);
    let rows = sqlx::query!(
        r#"
        SELECT
            (date_trunc('month', ts AT TIME ZONE 'UTC'))::date AS "month!",
            COUNT(*)                              AS "total!",
            COUNT(*) FILTER (WHERE status = 'up') AS "up!"
        FROM heartbeats
        WHERE monitor_id = $1 AND ts >= $2
        GROUP BY 1
        ORDER BY 1
        "#,
        monitor.0,
        since,
    )
    .fetch_all(pool)
    .await?;

    // Dense `months`-length output, oldest first. Today's month sits at
    // the last index.
    let now = OffsetDateTime::now_utc().date();
    let current_month_first =
        time::Date::from_calendar_date(now.year(), now.month(), 1).unwrap_or(now);
    let mut targets = Vec::with_capacity(months as usize);
    let mut y = current_month_first.year();
    let mut m_u8 = current_month_first.month() as u8;
    for _ in 0..months {
        let mth = time::Month::try_from(m_u8).unwrap_or(time::Month::January);
        targets.push(time::Date::from_calendar_date(y, mth, 1).unwrap_or(current_month_first));
        if m_u8 == 1 {
            m_u8 = 12;
            y -= 1;
        } else {
            m_u8 -= 1;
        }
    }
    targets.reverse();

    let mut out: Vec<MonthlyUptime> = targets
        .into_iter()
        .map(|d| MonthlyUptime {
            year_month: d,
            uptime_pct: None,
        })
        .collect();

    for r in rows {
        if let Some(idx) = out.iter().position(|m| m.year_month == r.month) {
            if r.total > 0 {
                out[idx].uptime_pct = Some(r.up as f32 / r.total as f32 * 100.0);
            }
        }
    }
    Ok(out)
}

/// One row of per-monitor rollup stats over the last `window_seconds`.
/// Monitors with no heartbeats in the window are absent from the result.
#[derive(Debug, Clone)]
pub struct MonitorSummary {
    pub monitor_id: MonitorId,
    pub total: i64,
    pub up: i64,
    pub avg_latency_ms: Option<f64>,
    pub last_status: Option<MonitorStatus>,
    pub last_ts: Option<OffsetDateTime>,
}

/// 24h-window rollup of every monitor that has heartbeats in the window.
/// One query — fine to call on every dashboard render.
pub async fn summary_window(pool: &DbPool, window_seconds: i64) -> DbResult<Vec<MonitorSummary>> {
    let since = OffsetDateTime::now_utc() - time::Duration::seconds(window_seconds);
    let rows = sqlx::query!(
        r#"
        WITH base AS (
            SELECT
                monitor_id,
                ts,
                status,
                latency_ms,
                ROW_NUMBER() OVER (PARTITION BY monitor_id ORDER BY ts DESC) AS rn
            FROM heartbeats
            WHERE ts >= $1
        )
        SELECT
            monitor_id,
            COUNT(*)::int8                                                            AS "total!",
            COUNT(*) FILTER (WHERE status = 'up')::int8                               AS "up!",
            AVG(latency_ms)
                FILTER (WHERE status = 'up' AND latency_ms IS NOT NULL)::float8       AS avg_latency_ms,
            (ARRAY_AGG(status     ORDER BY ts DESC))[1] AS "last_status: MonitorStatus",
            (ARRAY_AGG(ts         ORDER BY ts DESC))[1] AS last_ts
        FROM base
        GROUP BY monitor_id
        "#,
        since,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| MonitorSummary {
            monitor_id: MonitorId::from_uuid(r.monitor_id),
            total: r.total,
            up: r.up,
            avg_latency_ms: r.avg_latency_ms,
            last_status: r.last_status,
            last_ts: r.last_ts,
        })
        .collect())
}

/// Last `per_monitor` heartbeats for every monitor, oldest-first within each
/// monitor. One query for the dashboard history bars.
pub async fn recent_per_monitor(pool: &DbPool, per_monitor: i64) -> DbResult<Vec<Heartbeat>> {
    let rows = sqlx::query!(
        r#"
        SELECT
            monitor_id,
            ts,
            status AS "status: MonitorStatus",
            latency_ms,
            status_code,
            msg,
            retries,
            important
        FROM (
            SELECT
                monitor_id, ts, status, latency_ms, status_code, msg, retries, important,
                ROW_NUMBER() OVER (PARTITION BY monitor_id ORDER BY ts DESC) AS rn
            FROM heartbeats
        ) t
        WHERE rn <= $1
        ORDER BY monitor_id, ts ASC
        "#,
        per_monitor,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| Heartbeat {
            monitor_id: MonitorId::from_uuid(r.monitor_id),
            ts: r.ts,
            status: r.status,
            latency_ms: r.latency_ms,
            status_code: r.status_code,
            msg: r.msg,
            retries: r.retries,
            important: r.important,
        })
        .collect())
}
