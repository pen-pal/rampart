//! Heartbeat queries.
//!
//! Renamed from `checks.rs`. Inserts are bulk-batched by the scheduler;
//! reads are by monitor + recent time window for the dashboard.

use crate::{DbPool, DbResult};
use rampart_core::ids::OrgId;
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

/// Current rolling SLO uptime over the trailing `window_days`. Returns
/// `None` when the window holds zero heartbeats (caller renders "no
/// data" rather than a misleading 0%). Returns the percentage in the
/// same shape (0.0 — 100.0) as [`uptime_pct`], so frontends can compare
/// against `slo_target_pct` directly.
///
/// One simple aggregate query — same shape as `uptime_pct` with a
/// day-granularity window. Deliberately not a perfect SLI/SLO
/// error-budget calculator; just "you promised X%, you are at Y%".
pub async fn current_slo_uptime_pct(
    pool: &DbPool,
    monitor: MonitorId,
    window_days: i32,
) -> DbResult<Option<f64>> {
    let since = OffsetDateTime::now_utc() - time::Duration::days(window_days as i64);
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

/// Per-hour average latency for a single UTC calendar day. Returns a
/// sparse `(hour, avg_latency_ms, samples)` tuple per hour that recorded
/// at least one `status = 'up'` heartbeat. Hours with no successful
/// samples are absent from the result — the caller renders them as
/// no-data buckets.
///
/// Powers the per-day latency mini-chart on the public status page's
/// day-drilldown popover. Single GROUP BY query — cheap enough to call
/// on every popover open.
pub async fn day_hourly_latency(
    pool: &DbPool,
    monitor: MonitorId,
    day: time::Date,
) -> DbResult<Vec<(i32, Option<f32>, i32)>> {
    // Build the [day_start, day_end) UTC half-open window. `with_hms`
    // at (0,0,0) can never fail — those are always valid components.
    let day_start = day
        .with_hms(0, 0, 0)
        .expect("00:00:00 is always a valid time-of-day")
        .assume_utc();
    let day_end = day_start + time::Duration::hours(24);

    let rows = sqlx::query!(
        r#"
        SELECT
            EXTRACT(HOUR FROM date_trunc('hour', ts AT TIME ZONE 'UTC'))::int AS "hour!",
            AVG(latency_ms)::float4   AS avg_latency_ms,
            COUNT(*)::int             AS "samples!"
        FROM heartbeats
        WHERE monitor_id = $1
          AND ts >= $2
          AND ts <  $3
          AND status = 'up'
        GROUP BY 1
        ORDER BY 1
        "#,
        monitor.0,
        day_start,
        day_end,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| (r.hour, r.avg_latency_ms, r.samples))
        .collect())
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

// ── Set-based (batch) rollups ──────────────────────────────────────────
//
// The four functions below mirror `uptime_pct`, `avg_latency_ms`,
// `daily_status` and `monthly_uptime` exactly, but roll up MANY monitors in
// one query each (`WHERE monitor_id = ANY($1)` + `GROUP BY monitor_id`). They
// exist so the PUBLIC, unauthenticated status-page render path (status_pages::
// public_view) does a fixed handful of queries instead of ~6 per monitor —
// the query/DoS-amplification fix. Output is byte-identical to calling the
// per-monitor fns once per id, so a monitor absent from the returned map MUST
// be treated exactly as the per-monitor `None`/empty result by the caller.

use std::collections::HashMap;

/// Batch mirror of [`uptime_pct`]. Returns one entry per monitor that has at
/// least one heartbeat in the window; the value is `ok/total*100`. A monitor
/// with no heartbeats in the window is ABSENT from the map (NOT 0.0) — the
/// caller renders that exactly like the per-monitor `None`.
pub async fn uptime_pct_batch(
    pool: &DbPool,
    monitor_ids: &[Uuid],
    window_seconds: i64,
) -> DbResult<HashMap<Uuid, f64>> {
    if monitor_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let since = OffsetDateTime::now_utc() - time::Duration::seconds(window_seconds);
    let rows = sqlx::query!(
        r#"
        SELECT
            monitor_id,
            COUNT(*)                              AS "total!",
            COUNT(*) FILTER (WHERE status = 'up') AS "ok_count!"
        FROM heartbeats
        WHERE monitor_id = ANY($1) AND ts >= $2
        GROUP BY monitor_id
        "#,
        monitor_ids,
        since,
    )
    .fetch_all(pool)
    .await?;

    let mut out = HashMap::with_capacity(rows.len());
    for r in rows {
        // GROUP BY only emits rows for monitors with heartbeats, so total > 0
        // always holds here — matching the per-monitor `total == 0 → None`
        // guard, which simply leaves such monitors out of the map.
        if r.total > 0 {
            out.insert(r.monitor_id, r.ok_count as f64 / r.total as f64 * 100.0);
        }
    }
    Ok(out)
}

/// Batch mirror of [`avg_latency_ms`]. One entry per monitor that has at least
/// one `up` heartbeat with a non-null latency in the window; absent otherwise
/// (rendered as the per-monitor `None`).
pub async fn avg_latency_ms_batch(
    pool: &DbPool,
    monitor_ids: &[Uuid],
    window_seconds: i64,
) -> DbResult<HashMap<Uuid, f64>> {
    if monitor_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let since = OffsetDateTime::now_utc() - time::Duration::seconds(window_seconds);
    let rows = sqlx::query!(
        r#"
        SELECT
            monitor_id,
            AVG(latency_ms)::float8 AS avg
        FROM heartbeats
        WHERE monitor_id = ANY($1) AND ts >= $2 AND status = 'up' AND latency_ms IS NOT NULL
        GROUP BY monitor_id
        "#,
        monitor_ids,
        since,
    )
    .fetch_all(pool)
    .await?;

    let mut out = HashMap::with_capacity(rows.len());
    for r in rows {
        // The WHERE clause guarantees each grouped monitor has ≥1 qualifying
        // row, so AVG is non-null. Mirror the per-monitor fn (which returns
        // None when nothing qualifies) by only inserting present values.
        if let Some(avg) = r.avg {
            out.insert(r.monitor_id, avg);
        }
    }
    Ok(out)
}

/// Batch mirror of [`daily_status`]. Each value is a dense `days`-length
/// `Vec<u8>` of `u`/`d`/`w`/`m`/`n` chars, oldest day first (today's bucket
/// last) — identical to the per-monitor pivot. Every requested id is present
/// in the map; one with no heartbeats maps to `vec![b'n'; days]` (the same
/// value the per-monitor fn returns for an empty result).
pub async fn daily_status_batch(
    pool: &DbPool,
    monitor_ids: &[Uuid],
    days: i32,
) -> DbResult<HashMap<Uuid, Vec<u8>>> {
    // Seed every requested id with an all-'n' vector so monitors with zero
    // rows match the per-monitor empty-result behaviour exactly.
    let mut out: HashMap<Uuid, Vec<u8>> = monitor_ids
        .iter()
        .map(|id| (*id, vec![b'n'; days as usize]))
        .collect();
    if monitor_ids.is_empty() {
        return Ok(out);
    }
    let since = OffsetDateTime::now_utc() - time::Duration::days(days as i64);
    let rows = sqlx::query!(
        r#"
        SELECT
            monitor_id,
            (date_trunc('day', ts AT TIME ZONE 'UTC'))::date AS "day!",
            BOOL_OR(status = 'down')        AS "any_down!",
            BOOL_OR(status = 'warn')        AS "any_warn!",
            BOOL_OR(status != 'maintenance') AS "any_real!"
        FROM heartbeats
        WHERE monitor_id = ANY($1) AND ts >= $2
        GROUP BY monitor_id, (date_trunc('day', ts AT TIME ZONE 'UTC'))::date
        "#,
        monitor_ids,
        since,
    )
    .fetch_all(pool)
    .await?;

    // Same dense pivot as the per-monitor fn: today's bucket is the final
    // element; delta-days from today gives the index.
    let today = OffsetDateTime::now_utc().date();
    for r in rows {
        let delta = (today - r.day).whole_days();
        if delta < 0 || delta >= days as i64 {
            continue;
        }
        let idx = (days as i64 - 1 - delta) as usize;
        // Every monitor_id in `rows` came from `monitor_ids`, so the seed
        // entry always exists.
        if let Some(v) = out.get_mut(&r.monitor_id) {
            v[idx] = if r.any_down {
                b'd'
            } else if r.any_warn {
                b'w'
            } else if !r.any_real {
                b'm'
            } else {
                b'u'
            };
        }
    }
    Ok(out)
}

/// Batch mirror of [`monthly_uptime`]. Each value is a dense `months`-length
/// `Vec<MonthlyUptime>`, oldest month first (today's month last) — identical
/// to the per-monitor calendar-month construction. Every requested id is
/// present; one with no heartbeats maps to the dense vector with all
/// `uptime_pct: None`.
pub async fn monthly_uptime_batch(
    pool: &DbPool,
    monitor_ids: &[Uuid],
    months: i32,
) -> DbResult<HashMap<Uuid, Vec<MonthlyUptime>>> {
    // Build the shared dense target (oldest month first, today's month last)
    // once — it's identical across monitors. Each id gets its own clone with
    // all `uptime_pct: None`, mirroring the per-monitor empty result.
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
    let template: Vec<MonthlyUptime> = targets
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

    let since = OffsetDateTime::now_utc() - time::Duration::days((months as i64) * 32);
    let rows = sqlx::query!(
        r#"
        SELECT
            monitor_id,
            (date_trunc('month', ts AT TIME ZONE 'UTC'))::date AS "month!",
            COUNT(*)                              AS "total!",
            COUNT(*) FILTER (WHERE status = 'up') AS "up!"
        FROM heartbeats
        WHERE monitor_id = ANY($1) AND ts >= $2
        GROUP BY monitor_id, (date_trunc('month', ts AT TIME ZONE 'UTC'))::date
        "#,
        monitor_ids,
        since,
    )
    .fetch_all(pool)
    .await?;

    for r in rows {
        if let Some(v) = out.get_mut(&r.monitor_id) {
            if let Some(idx) = v.iter().position(|m| m.year_month == r.month) {
                if r.total > 0 {
                    v[idx].uptime_pct = Some(r.up as f32 / r.total as f32 * 100.0);
                }
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
pub async fn summary_window(
    pool: &DbPool,
    window_seconds: i64,
    org_id: OrgId,
) -> DbResult<Vec<MonitorSummary>> {
    let since = OffsetDateTime::now_utc() - time::Duration::seconds(window_seconds);
    let rows = sqlx::query!(
        r#"
        WITH base AS (
            SELECT
                h.monitor_id AS monitor_id,
                h.ts         AS ts,
                h.status     AS status,
                h.latency_ms AS latency_ms,
                ROW_NUMBER() OVER (PARTITION BY h.monitor_id ORDER BY h.ts DESC) AS rn
            FROM heartbeats h
            JOIN monitors m ON m.id = h.monitor_id
            WHERE h.ts >= $1 AND m.org_id = $2
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
        org_id.0,
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

/// MTBF / MTTR rollup over a trailing window.
///
/// * `mtbf_secs` — mean time between failures: total up-state seconds
///   divided by the number of up→down transitions. `None` when no
///   failures occurred in the window (nothing to average over).
/// * `mttr_secs` — mean time to recovery: total down-state seconds
///   divided by the number of down→up transitions. `None` when no
///   recoveries occurred in the window.
/// * `downtime_events` — count of up→down transitions (i.e. failures).
#[derive(Debug, Clone, serde::Serialize)]
pub struct MtbfMttr {
    pub mtbf_secs: Option<i64>,
    pub mttr_secs: Option<i64>,
    pub downtime_events: i64,
}

/// Compute MTBF + MTTR for a monitor over the trailing `window_seconds`.
///
/// Walks the heartbeat history in ascending-ts order, treating each
/// heartbeat as the start of a state segment that lasts until the next
/// heartbeat. Statuses other than `up`/`down` (warn, paused, pending,
/// maintenance) don't contribute to either bucket and don't count as
/// failures or recoveries — they're treated as gaps in the timeline.
///
/// Returns `MtbfMttr { mtbf_secs: None, mttr_secs: None, downtime_events: 0 }`
/// when there are no heartbeats or no transitions in the window.
pub async fn mtbf_mttr(
    pool: &DbPool,
    monitor: MonitorId,
    window_seconds: i64,
) -> DbResult<MtbfMttr> {
    let since = OffsetDateTime::now_utc() - time::Duration::seconds(window_seconds);
    let rows = sqlx::query!(
        r#"
        SELECT
            ts,
            status AS "status: MonitorStatus"
        FROM heartbeats
        WHERE monitor_id = $1 AND ts >= $2
        ORDER BY ts ASC
        "#,
        monitor.0,
        since,
    )
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        return Ok(MtbfMttr {
            mtbf_secs: None,
            mttr_secs: None,
            downtime_events: 0,
        });
    }

    // Walk consecutive heartbeats. Each pair (prev, curr) contributes a
    // segment of length `curr.ts - prev.ts` attributed to prev.status,
    // and the (prev.status -> curr.status) transition is recorded.
    let mut total_up_secs: i64 = 0;
    let mut total_down_secs: i64 = 0;
    let mut failures: i64 = 0; // up -> down transitions
    let mut recoveries: i64 = 0; // down -> up transitions

    for win in rows.windows(2) {
        let prev = &win[0];
        let curr = &win[1];
        let dur = (curr.ts - prev.ts).whole_seconds().max(0);
        match prev.status {
            MonitorStatus::Up => total_up_secs += dur,
            MonitorStatus::Down => total_down_secs += dur,
            _ => { /* warn/paused/pending/maintenance — not counted */ }
        }
        match (prev.status, curr.status) {
            (MonitorStatus::Up, MonitorStatus::Down) => failures += 1,
            (MonitorStatus::Down, MonitorStatus::Up) => recoveries += 1,
            _ => {}
        }
    }

    let mtbf_secs = if failures > 0 {
        Some(total_up_secs / failures)
    } else {
        None
    };
    let mttr_secs = if recoveries > 0 {
        Some(total_down_secs / recoveries)
    } else {
        None
    };

    Ok(MtbfMttr {
        mtbf_secs,
        mttr_secs,
        downtime_events: failures,
    })
}

/// SLO error-budget rollup for a single monitor over the configured
/// window. The "fuel gauge" surface on the Overview tab: shows the
/// operator how much downtime they have left to burn this window before
/// the SLO is breached.
///
/// * `allowed_downtime_secs` = `(100 - target_pct) / 100 * window_seconds`
/// * `used_downtime_secs`    = sum of heartbeat-derived down-segment
///   durations in the window (same walk as [`mtbf_mttr`])
/// * `remaining_downtime_secs` = `max(0, allowed - used)`
/// * `remaining_pct` = `(remaining / allowed) * 100`, or `100.0` when
///   `allowed` is zero (a 100% SLO leaves no budget — but the field
///   must still serialise as a finite number rather than NaN).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ErrorBudget {
    pub window_days: i32,
    pub target_pct: f64,
    pub allowed_downtime_secs: i64,
    pub used_downtime_secs: i64,
    pub remaining_downtime_secs: i64,
    pub remaining_pct: f64,
}

/// Compute the SLO error-budget for a monitor over `window_days` against
/// `target_pct`. Walks the heartbeat history in ascending-ts order (the
/// same approach used by [`mtbf_mttr`]) so down-segment durations come
/// from the timeline between consecutive heartbeats, not from a naive
/// `down_count * interval` approximation.
pub async fn error_budget(
    pool: &DbPool,
    monitor: MonitorId,
    window_days: i32,
    target_pct: f64,
) -> DbResult<ErrorBudget> {
    let window_seconds = (window_days as i64) * 86_400;
    let allowed_raw = ((100.0 - target_pct) / 100.0) * window_seconds as f64;
    let allowed = allowed_raw.round().max(0.0) as i64;

    let since = OffsetDateTime::now_utc() - time::Duration::seconds(window_seconds);
    let rows = sqlx::query!(
        r#"
        SELECT
            ts,
            status AS "status: MonitorStatus"
        FROM heartbeats
        WHERE monitor_id = $1 AND ts >= $2
        ORDER BY ts ASC
        "#,
        monitor.0,
        since,
    )
    .fetch_all(pool)
    .await?;

    // Sum down-segment durations the same way `mtbf_mttr` does: each
    // (prev, curr) pair contributes `curr.ts - prev.ts` to prev.status's
    // bucket. Only the `down` bucket matters here.
    let mut used: i64 = 0;
    for win in rows.windows(2) {
        let prev = &win[0];
        let curr = &win[1];
        if matches!(prev.status, MonitorStatus::Down) {
            let dur = (curr.ts - prev.ts).whole_seconds().max(0);
            used += dur;
        }
    }

    let remaining = (allowed - used).max(0);
    // A 100% SLO has no budget by definition; report 100 so the frontend
    // can render a full bar instead of NaN. Otherwise (remaining /
    // allowed) is a finite ratio.
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

/// One day's slice of the SLO error-budget burn-down. The frontend
/// plots `budget_remaining_pct` over `day` to show the budget depleting
/// across the window (a descending line from 100% toward 0%).
///
/// * `day` — the UTC calendar day this point summarises.
/// * `cumulative_down_secs` — total down-segment seconds from the start
///   of the window through the end of this day.
/// * `budget_remaining_secs` — `max(0, allowed_total - cumulative)`.
/// * `budget_remaining_pct` — `(remaining / allowed) * 100`, clamped to
///   `>= 0`; `100.0` when `allowed` is zero (a 100% SLO has no budget but
///   must still serialise as a finite number).
#[derive(Debug, Clone, serde::Serialize)]
pub struct BurndownPoint {
    pub day: time::Date,
    pub cumulative_down_secs: i64,
    pub budget_remaining_secs: i64,
    pub budget_remaining_pct: f64,
}

/// Day-by-day SLO error-budget burn-down over the trailing `window_days`
/// against `target_pct`. Returns one [`BurndownPoint`] per day in the
/// window, oldest first.
///
/// Down-seconds per day are derived the same way as [`error_budget`]:
/// walk the heartbeat history in ascending-ts order and attribute each
/// `(prev, curr)` segment's duration to `prev`'s day when `prev` is
/// `down`. The per-day buckets are then accumulated so each point's
/// `cumulative_down_secs` is the running total through that day, and the
/// remaining budget (`allowed_total - cumulative`) is reported alongside.
///
/// `allowed_total = (100 - target_pct) / 100 * window_days * 86_400` — the
/// same allowance [`error_budget`] reports as `allowed_downtime_secs`, so
/// the final burn-down point lines up with the point-in-time fuel gauge.
pub async fn error_budget_burndown(
    pool: &DbPool,
    monitor: MonitorId,
    window_days: i32,
    target_pct: f64,
) -> DbResult<Vec<BurndownPoint>> {
    let window_seconds = (window_days as i64) * 86_400;
    let allowed_raw = ((100.0 - target_pct) / 100.0) * window_seconds as f64;
    let allowed = allowed_raw.round().max(0.0) as i64;

    let since = OffsetDateTime::now_utc() - time::Duration::seconds(window_seconds);
    let rows = sqlx::query!(
        r#"
        SELECT
            ts,
            status AS "status: MonitorStatus"
        FROM heartbeats
        WHERE monitor_id = $1 AND ts >= $2
        ORDER BY ts ASC
        "#,
        monitor.0,
        since,
    )
    .fetch_all(pool)
    .await?;

    // Bucket down-segment seconds by the UTC calendar day the segment
    // started in. A segment is `prev.ts → curr.ts` attributed to
    // `prev.status`; only the `down` bucket matters here.
    use std::collections::BTreeMap;
    let mut per_day: BTreeMap<time::Date, i64> = BTreeMap::new();
    for win in rows.windows(2) {
        let prev = &win[0];
        let curr = &win[1];
        if matches!(prev.status, MonitorStatus::Down) {
            let dur = (curr.ts - prev.ts).whole_seconds().max(0);
            *per_day
                .entry(prev.ts.to_offset(time::UtcOffset::UTC).date())
                .or_insert(0) += dur;
        }
    }

    // Emit one dense point per day across the window, oldest first, with a
    // running cumulative total. `since.date()` is the oldest day; today is
    // the last point. The number of days spanned is `(today - first_day)`
    // inclusive of both ends.
    let first_day = since.date();
    let today = OffsetDateTime::now_utc().date();
    let span = (today - first_day).whole_days().max(0);
    let mut out: Vec<BurndownPoint> = Vec::with_capacity(span as usize + 1);
    let mut cumulative: i64 = 0;
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

/// Last `per_monitor` heartbeats for every monitor, oldest-first within each
/// monitor. One query for the dashboard history bars.
pub async fn recent_per_monitor(
    pool: &DbPool,
    per_monitor: i64,
    org_id: OrgId,
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
        FROM (
            SELECT
                h.monitor_id, h.ts, h.status, h.latency_ms, h.status_code, h.msg, h.retries, h.important,
                ROW_NUMBER() OVER (PARTITION BY h.monitor_id ORDER BY h.ts DESC) AS rn
            FROM heartbeats h
            JOIN monitors m ON m.id = h.monitor_id
            WHERE m.org_id = $2
        ) t
        WHERE rn <= $1
        ORDER BY monitor_id, ts ASC
        "#,
        per_monitor,
        org_id.0,
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
