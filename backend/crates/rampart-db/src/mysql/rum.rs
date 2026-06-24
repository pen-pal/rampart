//! MySQL `rum` domain — Real User Monitoring. Ported from PG.
//!
//! Dialect: uuid→CHAR(36), ts→BIGINT, `make_interval(hours=>?)` → Rust-computed
//! cutoff, `ILIKE` → app-side lowercase `contains`. **The p75 reads differ:**
//! PG uses `percentile_cont(0.75) WITHIN GROUP` as an aggregate, which MySQL
//! lacks (MariaDB only as a window function). So the breakdown reads fetch the
//! windowed rows and aggregate (group + count + p75 + last_seen) in Rust.
//!
//! ponytail: app-side aggregation is bounded by short RUM retention × the
//! windowed read; if beacon volume outgrows that, push p75 down to MariaDB's
//! `PERCENTILE_CONT(...) OVER (...)` window function with a DISTINCT wrapper.

use super::ts;
use crate::rum::{RumBrowser, RumSample, RumUser};
use crate::DbResult;
use rampart_core::ids::OrgId;
use rampart_core::rum::{RumBeacon, RumPage, RumTracedLoad, RumVitals};
use sqlx::{MySqlPool, Row};
use std::collections::HashMap;
use time::OffsetDateTime;
use uuid::Uuid;

/// Linear-interpolation percentile (matches PG `percentile_cont`). NULLs already
/// filtered by the caller. `None` for an empty set.
fn p75(mut v: Vec<f64>) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let rank = 0.75 * (v.len() - 1) as f64;
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    let frac = rank - lo as f64;
    Some(v[lo] + frac * (v[hi] - v[lo]))
}

/// Coarse browser family from the UA — mirrors the PG `ILIKE` CASE (order
/// matters: Edge/Opera UAs also contain "chrome").
fn browser_of(ua: Option<&str>) -> &'static str {
    match ua {
        None => "Unknown",
        Some(u) => {
            let u = u.to_ascii_lowercase();
            if u.contains("edg/") || u.contains("edge") {
                "Edge"
            } else if u.contains("opr/") || u.contains("opera") {
                "Opera"
            } else if u.contains("chrome") || u.contains("crios") {
                "Chrome"
            } else if u.contains("firefox") || u.contains("fxios") {
                "Firefox"
            } else if u.contains("safari") {
                "Safari"
            } else {
                "Other"
            }
        }
    }
}

fn cutoff_hours(hours: i32, max: i32) -> i64 {
    OffsetDateTime::now_utc().unix_timestamp() - hours.clamp(1, max) as i64 * 3600
}

pub async fn insert_event(pool: &MySqlPool, b: &RumBeacon, org_id: OrgId) -> DbResult<()> {
    sqlx::query(
        "INSERT INTO rum_events
            (id, app, url, session, ua, trace_id, user_id,
             lcp_ms, fcp_ms, cls, inp_ms, ttfb_ms, load_ms, org_id)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(Uuid::now_v7().to_string())
    .bind(&b.app)
    .bind(&b.url)
    .bind(&b.session)
    .bind(&b.ua)
    .bind(&b.trace_id)
    .bind(&b.user_id)
    .bind(b.metrics.lcp)
    .bind(b.metrics.fcp)
    .bind(b.metrics.cls)
    .bind(b.metrics.inp)
    .bind(b.metrics.ttfb)
    .bind(b.metrics.load)
    .bind(org_id.0.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn page_samples(
    pool: &MySqlPool,
    app: Option<&str>,
    url: &str,
    hours: i32,
    limit: i64,
    org_id: OrgId,
) -> DbResult<Vec<RumSample>> {
    let rows = sqlx::query(
        "SELECT ts, user_id, session, ua, trace_id, lcp_ms, inp_ms, cls, load_ms
         FROM rum_events
         WHERE url = ? AND received_at > ? AND (? IS NULL OR app = ?) AND org_id = ?
         ORDER BY ts DESC LIMIT ?",
    )
    .bind(url)
    .bind(cutoff_hours(hours, 2160))
    .bind(app)
    .bind(app)
    .bind(org_id.0.to_string())
    .bind(limit.clamp(1, 200))
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| RumSample {
            ts: ts(r.get::<i64, _>("ts")),
            user_id: r.get("user_id"),
            session: r.get("session"),
            ua: r.get("ua"),
            trace_id: r.get("trace_id"),
            lcp_ms: r.get("lcp_ms"),
            inp_ms: r.get("inp_ms"),
            cls: r.get("cls"),
            load_ms: r.get("load_ms"),
        })
        .collect())
}

pub async fn recent_traced(
    pool: &MySqlPool,
    app: Option<&str>,
    hours: i32,
    limit: i64,
    org_id: OrgId,
) -> DbResult<Vec<RumTracedLoad>> {
    let rows = sqlx::query(
        "SELECT url, trace_id, load_ms, ts
         FROM rum_events
         WHERE trace_id IS NOT NULL AND received_at > ? AND (? IS NULL OR app = ?) AND org_id = ?
         ORDER BY ts DESC LIMIT ?",
    )
    .bind(cutoff_hours(hours, 24 * 90))
    .bind(app)
    .bind(app)
    .bind(org_id.0.to_string())
    .bind(limit.clamp(1, 200))
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .filter_map(|r| {
            r.get::<Option<String>, _>("trace_id")
                .map(|trace_id| RumTracedLoad {
                    url: r.get("url"),
                    trace_id,
                    load_ms: r.get("load_ms"),
                    ts: ts(r.get::<i64, _>("ts")),
                })
        })
        .collect())
}

pub async fn summary(
    pool: &MySqlPool,
    app: Option<&str>,
    hours: i32,
    org_id: OrgId,
) -> DbResult<RumVitals> {
    let rows = sqlx::query(
        "SELECT lcp_ms, fcp_ms, cls, inp_ms, ttfb_ms, load_ms
         FROM rum_events
         WHERE (? IS NULL OR app = ?) AND received_at > ? AND org_id = ?",
    )
    .bind(app)
    .bind(app)
    .bind(cutoff_hours(hours, 2160))
    .bind(org_id.0.to_string())
    .fetch_all(pool)
    .await?;
    let col = |name: &str| {
        rows.iter()
            .filter_map(|r| r.get::<Option<f64>, _>(name))
            .collect::<Vec<_>>()
    };
    Ok(RumVitals {
        views: rows.len() as i64,
        lcp_p75: p75(col("lcp_ms")),
        fcp_p75: p75(col("fcp_ms")),
        cls_p75: p75(col("cls")),
        inp_p75: p75(col("inp_ms")),
        ttfb_p75: p75(col("ttfb_ms")),
        load_p75: p75(col("load_ms")),
    })
}

pub async fn pages(
    pool: &MySqlPool,
    app: Option<&str>,
    hours: i32,
    org_id: OrgId,
) -> DbResult<Vec<RumPage>> {
    let rows = sqlx::query(
        "SELECT app, url, lcp_ms, inp_ms, cls, received_at
         FROM rum_events
         WHERE (? IS NULL OR app = ?) AND received_at > ? AND org_id = ?",
    )
    .bind(app)
    .bind(app)
    .bind(cutoff_hours(hours, 2160))
    .bind(org_id.0.to_string())
    .fetch_all(pool)
    .await?;

    // group by (app, url): count + p75(lcp/inp/cls) + max(received_at).
    struct Acc {
        app: String,
        views: i64,
        lcp: Vec<f64>,
        inp: Vec<f64>,
        cls: Vec<f64>,
        last_seen: i64,
    }
    let mut by: HashMap<(String, String), Acc> = HashMap::new();
    for r in &rows {
        let app: String = r.get("app");
        let url: String = r.get("url");
        let recv = r.get::<i64, _>("received_at");
        let e = by.entry((app.clone(), url)).or_insert_with(|| Acc {
            app,
            views: 0,
            lcp: vec![],
            inp: vec![],
            cls: vec![],
            last_seen: 0,
        });
        e.views += 1;
        if let Some(v) = r.get::<Option<f64>, _>("lcp_ms") {
            e.lcp.push(v);
        }
        if let Some(v) = r.get::<Option<f64>, _>("inp_ms") {
            e.inp.push(v);
        }
        if let Some(v) = r.get::<Option<f64>, _>("cls") {
            e.cls.push(v);
        }
        e.last_seen = e.last_seen.max(recv);
    }
    let mut out: Vec<RumPage> = by
        .into_iter()
        .map(|((_, url), a)| RumPage {
            app: a.app,
            url,
            views: a.views,
            lcp_p75: p75(a.lcp),
            inp_p75: p75(a.inp),
            cls_p75: p75(a.cls),
            last_seen: ts(a.last_seen),
        })
        .collect();
    out.sort_by_key(|b| std::cmp::Reverse(b.views));
    out.truncate(200);
    Ok(out)
}

pub async fn browser_breakdown(
    pool: &MySqlPool,
    app: Option<&str>,
    hours: i32,
    org_id: OrgId,
) -> DbResult<Vec<RumBrowser>> {
    let rows = sqlx::query(
        "SELECT ua, lcp_ms FROM rum_events
         WHERE (? IS NULL OR app = ?) AND received_at > ? AND org_id = ?",
    )
    .bind(app)
    .bind(app)
    .bind(cutoff_hours(hours, 2160))
    .bind(org_id.0.to_string())
    .fetch_all(pool)
    .await?;
    let mut by: HashMap<&'static str, (i64, Vec<f64>)> = HashMap::new();
    for r in &rows {
        let b = browser_of(r.get::<Option<String>, _>("ua").as_deref());
        let e = by.entry(b).or_insert((0, vec![]));
        e.0 += 1;
        if let Some(v) = r.get::<Option<f64>, _>("lcp_ms") {
            e.1.push(v);
        }
    }
    let mut out: Vec<RumBrowser> = by
        .into_iter()
        .map(|(browser, (views, lcp))| RumBrowser {
            browser: browser.to_string(),
            views,
            lcp_p75: p75(lcp),
        })
        .collect();
    out.sort_by_key(|b| std::cmp::Reverse(b.views));
    Ok(out)
}

pub async fn user_breakdown(
    pool: &MySqlPool,
    app: Option<&str>,
    hours: i32,
    org_id: OrgId,
) -> DbResult<Vec<RumUser>> {
    let rows = sqlx::query(
        "SELECT user_id, lcp_ms FROM rum_events
         WHERE user_id IS NOT NULL AND (? IS NULL OR app = ?) AND received_at > ? AND org_id = ?",
    )
    .bind(app)
    .bind(app)
    .bind(cutoff_hours(hours, 2160))
    .bind(org_id.0.to_string())
    .fetch_all(pool)
    .await?;
    let mut by: HashMap<String, (i64, Vec<f64>)> = HashMap::new();
    for r in &rows {
        let Some(uid) = r.get::<Option<String>, _>("user_id") else {
            continue;
        };
        let e = by.entry(uid).or_insert((0, vec![]));
        e.0 += 1;
        if let Some(v) = r.get::<Option<f64>, _>("lcp_ms") {
            e.1.push(v);
        }
    }
    let mut out: Vec<RumUser> = by
        .into_iter()
        .map(|(user_id, (views, lcp))| RumUser {
            user_id,
            views,
            lcp_p75: p75(lcp),
        })
        .collect();
    out.sort_by_key(|b| std::cmp::Reverse(b.views));
    out.truncate(50);
    Ok(out)
}

pub async fn apps(pool: &MySqlPool, org_id: OrgId) -> DbResult<Vec<String>> {
    let cutoff = OffsetDateTime::now_utc().unix_timestamp() - 7 * 86400;
    let rows = sqlx::query(
        "SELECT DISTINCT app FROM rum_events
         WHERE received_at > ? AND org_id = ? ORDER BY app LIMIT 200",
    )
    .bind(cutoff)
    .bind(org_id.0.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(|r| r.get::<String, _>("app")).collect())
}

pub async fn prune(pool: &MySqlPool, days: i32) -> DbResult<u64> {
    let cutoff = OffsetDateTime::now_utc().unix_timestamp() - days.max(0) as i64 * 86400;
    super::chunked_delete_older(pool, "rum_events", "received_at", cutoff).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use rampart_core::rum::RumMetrics;

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    fn beacon(url: &str, lcp: f64, ua: &str, user: Option<&str>, trace: Option<&str>) -> RumBeacon {
        RumBeacon {
            app: "web".into(),
            url: url.into(),
            session: Some("s1".into()),
            ua: Some(ua.into()),
            trace_id: trace.map(String::from),
            user_id: user.map(String::from),
            metrics: RumMetrics {
                lcp: Some(lcp),
                fcp: Some(lcp / 2.0),
                cls: Some(0.1),
                inp: Some(50.0),
                ttfb: Some(20.0),
                load: Some(lcp * 2.0),
            },
        }
    }

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn ingest_and_aggregates(pool: MySqlPool) {
        let org = super::super::oid(DEF);
        // 4 beacons on /home (varying lcp), 1 on /about, 1 Firefox + a traced one.
        for lcp in [100.0, 200.0, 300.0, 400.0] {
            insert_event(
                &pool,
                &beacon("/home", lcp, "Mozilla Chrome/120", Some("u1"), None),
                org,
            )
            .await
            .unwrap();
        }
        insert_event(
            &pool,
            &beacon(
                "/about",
                500.0,
                "Mozilla Firefox/120",
                Some("u2"),
                Some("abc123"),
            ),
            org,
        )
        .await
        .unwrap();

        // summary p75 across all 5 lcp = [100,200,300,400,500] → 400.
        let s = summary(&pool, None, 24, org).await.unwrap();
        assert_eq!(s.views, 5);
        assert_eq!(s.lcp_p75, Some(400.0));

        // pages: /home busiest (4), /about (1); p75 of /home lcp = 325.
        let pgs = pages(&pool, None, 24, org).await.unwrap();
        assert_eq!(pgs.len(), 2);
        assert_eq!(pgs[0].url, "/home");
        assert_eq!(pgs[0].views, 4);
        assert_eq!(pgs[0].lcp_p75, Some(325.0));

        // page drill-down + traced feed.
        assert_eq!(
            page_samples(&pool, None, "/home", 24, 50, org)
                .await
                .unwrap()
                .len(),
            4
        );
        let traced = recent_traced(&pool, None, 24, 50, org).await.unwrap();
        assert_eq!(traced.len(), 1);
        assert_eq!(traced[0].trace_id, "abc123");

        // browser + user breakdowns.
        let br = browser_breakdown(&pool, None, 24, org).await.unwrap();
        assert_eq!(br[0].browser, "Chrome"); // 4 views, busiest
        assert!(br.iter().any(|b| b.browser == "Firefox"));
        let us = user_breakdown(&pool, None, 24, org).await.unwrap();
        assert_eq!(us[0].user_id, "u1");
        assert_eq!(us[0].views, 4);

        // apps dropdown + cross-org isolation.
        assert_eq!(apps(&pool, org).await.unwrap(), vec!["web".to_string()]);
        let other = super::super::orgs::create(&pool, "other", "Other")
            .await
            .unwrap();
        assert_eq!(summary(&pool, None, 24, other.id).await.unwrap().views, 0);

        // prune: backdate all → wiped.
        assert_eq!(prune(&pool, 1).await.unwrap(), 0);
        sqlx::query("UPDATE rum_events SET received_at = ?")
            .bind(OffsetDateTime::now_utc().unix_timestamp() - 10 * 86400)
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(prune(&pool, 1).await.unwrap(), 5);
    }
}
