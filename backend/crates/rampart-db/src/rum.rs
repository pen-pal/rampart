//! RUM storage (migration 0080).
//!
//! [`insert_event`] stores one page-view beacon. Reads aggregate p75 per
//! metric (the Web Vitals statistic) overall and per URL. Events age out via
//! [`prune`]. Beacon parsing lives in `rampart_core::rum`.

use crate::{DbPool, DbResult};
use rampart_core::rum::{RumBeacon, RumPage, RumTracedLoad, RumVitals};
use uuid::Uuid;

/// Store one cleaned beacon.
pub async fn insert_event(pool: &DbPool, b: &RumBeacon) -> DbResult<()> {
    sqlx::query!(
        r#"
        INSERT INTO rum_events
            (id, app, url, session, ua, trace_id, lcp_ms, fcp_ms, cls, inp_ms, ttfb_ms, load_ms)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
        "#,
        Uuid::now_v7(),
        b.app,
        b.url,
        b.session,
        b.ua,
        b.trace_id,
        b.metrics.lcp,
        b.metrics.fcp,
        b.metrics.cls,
        b.metrics.inp,
        b.metrics.ttfb,
        b.metrics.load,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Recent page-loads that carried a backend trace id — the RUM → trace feed.
pub async fn recent_traced(
    pool: &DbPool,
    app: Option<&str>,
    hours: i32,
    limit: i64,
) -> DbResult<Vec<RumTracedLoad>> {
    let rows = sqlx::query!(
        r#"
        SELECT url, trace_id AS "trace_id!", load_ms, ts
        FROM rum_events
        WHERE trace_id IS NOT NULL
          AND received_at > now() - make_interval(hours => $1)
          AND ($2::text IS NULL OR app = $2)
        ORDER BY ts DESC
        LIMIT $3
        "#,
        hours.clamp(1, 24 * 90),
        app,
        limit.clamp(1, 200),
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| RumTracedLoad {
            url: r.url,
            trace_id: r.trace_id,
            load_ms: r.load_ms,
            ts: r.ts,
        })
        .collect())
}

/// p75 of each metric over the window (optionally filtered to one app).
pub async fn summary(pool: &DbPool, app: Option<&str>, hours: i32) -> DbResult<RumVitals> {
    let r = sqlx::query!(
        r#"
        SELECT
            count(*) AS "views!",
            percentile_cont(0.75) WITHIN GROUP (ORDER BY lcp_ms)  AS "lcp_p75",
            percentile_cont(0.75) WITHIN GROUP (ORDER BY fcp_ms)  AS "fcp_p75",
            percentile_cont(0.75) WITHIN GROUP (ORDER BY cls)     AS "cls_p75",
            percentile_cont(0.75) WITHIN GROUP (ORDER BY inp_ms)  AS "inp_p75",
            percentile_cont(0.75) WITHIN GROUP (ORDER BY ttfb_ms) AS "ttfb_p75",
            percentile_cont(0.75) WITHIN GROUP (ORDER BY load_ms) AS "load_p75"
        FROM rum_events
        WHERE ($1::text IS NULL OR app = $1)
          AND received_at > now() - make_interval(hours => $2)
        "#,
        app,
        hours.clamp(1, 2160),
    )
    .fetch_one(pool)
    .await?;
    Ok(RumVitals {
        views: r.views,
        lcp_p75: r.lcp_p75,
        fcp_p75: r.fcp_p75,
        cls_p75: r.cls_p75,
        inp_p75: r.inp_p75,
        ttfb_p75: r.ttfb_p75,
        load_p75: r.load_p75,
    })
}

/// Per-URL rollup, busiest first.
pub async fn pages(pool: &DbPool, app: Option<&str>, hours: i32) -> DbResult<Vec<RumPage>> {
    let rows = sqlx::query!(
        r#"
        SELECT
            url AS "url!",
            count(*) AS "views!",
            percentile_cont(0.75) WITHIN GROUP (ORDER BY lcp_ms) AS "lcp_p75",
            percentile_cont(0.75) WITHIN GROUP (ORDER BY inp_ms) AS "inp_p75",
            percentile_cont(0.75) WITHIN GROUP (ORDER BY cls)    AS "cls_p75",
            max(received_at) AS "last_seen!"
        FROM rum_events
        WHERE ($1::text IS NULL OR app = $1)
          AND received_at > now() - make_interval(hours => $2)
        GROUP BY url
        ORDER BY count(*) DESC
        LIMIT 200
        "#,
        app,
        hours.clamp(1, 2160),
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| RumPage {
            url: r.url,
            views: r.views,
            lcp_p75: r.lcp_p75,
            inp_p75: r.inp_p75,
            cls_p75: r.cls_p75,
            last_seen: r.last_seen,
        })
        .collect())
}

/// Distinct app/site names seen recently (filter dropdown).
pub async fn apps(pool: &DbPool) -> DbResult<Vec<String>> {
    let rows = sqlx::query_scalar!(
        r#"
        SELECT DISTINCT app AS "app!"
        FROM rum_events
        WHERE received_at > now() - make_interval(days => 7)
        ORDER BY app
        LIMIT 200
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Delete events older than `days`.
pub async fn prune(pool: &DbPool, days: i32) -> DbResult<u64> {
    let result = sqlx::query!(
        "DELETE FROM rum_events WHERE received_at < now() - make_interval(days => $1)",
        days,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
