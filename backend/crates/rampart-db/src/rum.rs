//! RUM storage (migration 0080).
//!
//! [`insert_event`] stores one page-view beacon. Reads aggregate p75 per
//! metric (the Web Vitals statistic) overall and per URL. Events age out via
//! [`prune`]. Beacon parsing lives in `rampart_core::rum`.

use crate::{DbPool, DbResult};
use rampart_core::ids::OrgId;
use rampart_core::rum::{RumBeacon, RumPage, RumTracedLoad, RumVitals};
use uuid::Uuid;

/// Store one cleaned beacon.
pub async fn insert_event(
    pool: &DbPool,
    b: &RumBeacon,
    org_id: rampart_core::ids::OrgId,
) -> DbResult<()> {
    sqlx::query!(
        r#"
        INSERT INTO rum_events
            (id, app, url, session, ua, trace_id, user_id, lcp_ms, fcp_ms, cls, inp_ms, ttfb_ms, load_ms, org_id)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
        "#,
        Uuid::now_v7(),
        b.app,
        b.url,
        b.session,
        b.ua,
        b.trace_id,
        b.user_id,
        b.metrics.lcp,
        b.metrics.fcp,
        b.metrics.cls,
        b.metrics.inp,
        b.metrics.ttfb,
        b.metrics.load,
        org_id.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// One recent page-load for the per-URL drill-down: who/what/when + vitals.
#[derive(Debug, serde::Serialize)]
pub struct RumSample {
    pub ts: time::OffsetDateTime,
    pub user_id: Option<String>,
    pub session: Option<String>,
    pub ua: Option<String>,
    pub trace_id: Option<String>,
    pub lcp_ms: Option<f64>,
    pub inp_ms: Option<f64>,
    pub cls: Option<f64>,
    pub load_ms: Option<f64>,
}

/// Recent individual beacons for one URL — the page drill-down. Newest first.
pub async fn page_samples(
    pool: &DbPool,
    app: Option<&str>,
    url: &str,
    hours: i32,
    limit: i64,
    org_id: OrgId,
) -> DbResult<Vec<RumSample>> {
    let rows = sqlx::query!(
        r#"
        SELECT ts, user_id, session, ua, trace_id, lcp_ms, inp_ms, cls, load_ms
        FROM rum_events
        WHERE url = $1
          AND received_at > now() - make_interval(hours => $2)
          AND ($3::text IS NULL OR app = $3)
          AND org_id = $5
        ORDER BY ts DESC
        LIMIT $4
        "#,
        url,
        hours.clamp(1, 2160),
        app,
        limit.clamp(1, 200),
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| RumSample {
            ts: r.ts,
            user_id: r.user_id,
            session: r.session,
            ua: r.ua,
            trace_id: r.trace_id,
            lcp_ms: r.lcp_ms,
            inp_ms: r.inp_ms,
            cls: r.cls,
            load_ms: r.load_ms,
        })
        .collect())
}

/// Recent page-loads that carried a backend trace id — the RUM → trace feed.
pub async fn recent_traced(
    pool: &DbPool,
    app: Option<&str>,
    hours: i32,
    limit: i64,
    org_id: OrgId,
) -> DbResult<Vec<RumTracedLoad>> {
    let rows = sqlx::query!(
        r#"
        SELECT url, trace_id AS "trace_id!", load_ms, ts
        FROM rum_events
        WHERE trace_id IS NOT NULL
          AND received_at > now() - make_interval(hours => $1)
          AND ($2::text IS NULL OR app = $2)
          AND org_id = $4
        ORDER BY ts DESC
        LIMIT $3
        "#,
        hours.clamp(1, 24 * 90),
        app,
        limit.clamp(1, 200),
        org_id.0,
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
pub async fn summary(
    pool: &DbPool,
    app: Option<&str>,
    hours: i32,
    org_id: OrgId,
) -> DbResult<RumVitals> {
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
          AND org_id = $3
        "#,
        app,
        hours.clamp(1, 2160),
        org_id.0,
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
pub async fn pages(
    pool: &DbPool,
    app: Option<&str>,
    hours: i32,
    org_id: OrgId,
) -> DbResult<Vec<RumPage>> {
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
          AND org_id = $3
        GROUP BY url
        ORDER BY count(*) DESC
        LIMIT 200
        "#,
        app,
        hours.clamp(1, 2160),
        org_id.0,
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

/// Page-views + p75 LCP grouped by coarse browser family — the "which browser
/// is slow" RUM breakdown. UA classification is deliberately coarse (a handful
/// of ILIKE buckets); good enough to spot a problem browser without a UA-parser
/// dependency.
#[derive(Debug, serde::Serialize)]
pub struct RumBrowser {
    pub browser: String,
    pub views: i64,
    pub lcp_p75: Option<f64>,
}

pub async fn browser_breakdown(
    pool: &DbPool,
    app: Option<&str>,
    hours: i32,
    org_id: OrgId,
) -> DbResult<Vec<RumBrowser>> {
    let rows = sqlx::query!(
        r#"
        SELECT
            CASE
                WHEN ua ILIKE '%edg/%' OR ua ILIKE '%edge%' THEN 'Edge'
                WHEN ua ILIKE '%opr/%' OR ua ILIKE '%opera%' THEN 'Opera'
                WHEN ua ILIKE '%chrome%' OR ua ILIKE '%crios%' THEN 'Chrome'
                WHEN ua ILIKE '%firefox%' OR ua ILIKE '%fxios%' THEN 'Firefox'
                WHEN ua ILIKE '%safari%' THEN 'Safari'
                WHEN ua IS NULL THEN 'Unknown'
                ELSE 'Other'
            END AS "browser!",
            count(*) AS "views!",
            percentile_cont(0.75) WITHIN GROUP (ORDER BY lcp_ms) AS "lcp_p75"
        FROM rum_events
        WHERE ($1::text IS NULL OR app = $1)
          AND received_at > now() - make_interval(hours => $2)
          AND org_id = $3
        GROUP BY 1
        ORDER BY count(*) DESC
        "#,
        app,
        hours.clamp(1, 2160),
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| RumBrowser {
            browser: r.browser,
            views: r.views,
            lcp_p75: r.lcp_p75,
        })
        .collect())
}

/// Page-views + p75 LCP grouped by app user — "which users have a bad
/// experience". Only beacons that carried a `user_id`. Busiest first.
#[derive(Debug, serde::Serialize)]
pub struct RumUser {
    pub user_id: String,
    pub views: i64,
    pub lcp_p75: Option<f64>,
}

pub async fn user_breakdown(
    pool: &DbPool,
    app: Option<&str>,
    hours: i32,
    org_id: OrgId,
) -> DbResult<Vec<RumUser>> {
    let rows = sqlx::query!(
        r#"
        SELECT user_id AS "user_id!", count(*) AS "views!",
               percentile_cont(0.75) WITHIN GROUP (ORDER BY lcp_ms) AS "lcp_p75"
        FROM rum_events
        WHERE user_id IS NOT NULL
          AND ($1::text IS NULL OR app = $1)
          AND received_at > now() - make_interval(hours => $2)
          AND org_id = $3
        GROUP BY user_id
        ORDER BY count(*) DESC
        LIMIT 50
        "#,
        app,
        hours.clamp(1, 2160),
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| RumUser {
            user_id: r.user_id,
            views: r.views,
            lcp_p75: r.lcp_p75,
        })
        .collect())
}

/// Distinct app/site names seen recently (filter dropdown).
pub async fn apps(pool: &DbPool, org_id: OrgId) -> DbResult<Vec<String>> {
    let rows = sqlx::query_scalar!(
        r#"
        SELECT DISTINCT app AS "app!"
        FROM rum_events
        WHERE received_at > now() - make_interval(days => 7)
          AND org_id = $1
        ORDER BY app
        LIMIT 200
        "#,
        org_id.0,
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
