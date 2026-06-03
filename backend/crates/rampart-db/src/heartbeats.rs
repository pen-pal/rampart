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
