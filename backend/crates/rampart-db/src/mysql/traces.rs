//! MySQL `traces` domain — span storage + trace assembly (telemetry foundation
//! 2/2). Mirrors the PG/SQLite free-fn surface: insert_spans / list_traces /
//! get_trace_spans / service_map / operation_stats / operation_trend / prune.
//!
//! MySQL has no `percentile_cont`/`LATERAL`/`ARRAY_AGG`, so the four analytic
//! reads fetch the span rows and aggregate in Rust — including a continuous
//! percentile (`p_cont`) matching PG's `percentile_cont` (identical to SQLite).
//! insert/get/prune are plain SQL. Dialect: hex ids→VARCHAR, ns→BIGINT,
//! double→DOUBLE, jsonb→LONGTEXT, received_at→BIGINT; `UNNEST … ON
//! CONFLICT(span_id) DO NOTHING` → per-row tx with `INSERT IGNORE` (a dup
//! span_id contributes 0 to rows_affected → exact insert count on retransmit);
//! `(received_at-origin)/step` bucket → `DIV` integer math.

use super::ts;
use crate::traces::TraceFilter;
use crate::{DbError, DbResult};
use rampart_core::ids::OrgId;
use rampart_core::trace::{ParsedSpan, ServiceEdge, Span, TraceSummary};
use rampart_core::OperationStat;
use sqlx::{MySqlPool, Row};
use std::collections::{BTreeMap, BTreeSet};
use time::OffsetDateTime;

/// Continuous percentile (matches PG `percentile_cont`): rank = p·(n−1), linear
/// interpolation between the bracketing samples. `None` when empty.
fn p_cont(vals: &mut [f64], p: f64) -> Option<f64> {
    if vals.is_empty() {
        return None;
    }
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = vals.len();
    if n == 1 {
        return Some(vals[0]);
    }
    let rank = p * (n as f64 - 1.0);
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    let frac = rank - lo as f64;
    Some(vals[lo] + (vals[hi] - vals[lo]) * frac)
}

fn now_unix() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

/// Bulk-insert spans (one tx; PG uses UNNEST), de-duped on span_id.
pub async fn insert_spans(pool: &MySqlPool, spans: &[ParsedSpan], org_id: OrgId) -> DbResult<u64> {
    if spans.is_empty() {
        return Ok(0);
    }
    let org = org_id.0.to_string();
    let mut tx = pool.begin().await?;
    let mut n = 0u64;
    for s in spans {
        // INSERT IGNORE (not ON DUPLICATE KEY UPDATE col=col): a duplicate span_id
        // contributes 0 to rows_affected so the inserted-count stays EXACT. The
        // no-op-UPDATE form returns 1 (matched) on MariaDB, over-counting dedups.
        let res = sqlx::query(
            "INSERT IGNORE INTO spans
                (span_id, trace_id, parent_span_id, service_name, name, kind, start_ns, end_ns,
                 duration_ms, status_code, status_message, attributes, org_id)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&s.span_id)
        .bind(&s.trace_id)
        .bind(&s.parent_span_id)
        .bind(&s.service_name)
        .bind(&s.name)
        .bind(s.kind)
        .bind(s.start_ns)
        .bind(s.end_ns)
        .bind(s.duration_ms())
        .bind(s.status_code)
        .bind(&s.status_message)
        .bind(serde_json::to_string(&s.attributes).unwrap_or_else(|_| "null".into()))
        .bind(&org)
        .execute(&mut *tx)
        .await?;
        n += res.rows_affected();
    }
    tx.commit().await?;
    Ok(n)
}

fn span_from(r: &sqlx::mysql::MySqlRow) -> Span {
    Span {
        span_id: r.get("span_id"),
        trace_id: r.get("trace_id"),
        parent_span_id: r.get("parent_span_id"),
        service_name: r.get("service_name"),
        name: r.get("name"),
        kind: r.get::<i16, _>("kind"),
        start_ns: r.get("start_ns"),
        end_ns: r.get("end_ns"),
        duration_ms: r.get("duration_ms"),
        status_code: r.get::<i16, _>("status_code"),
        status_message: r.get("status_message"),
        attributes: r
            .get::<Option<String>, _>("attributes")
            .and_then(|s| serde_json::from_str(&s).ok()),
        received_at: ts(r.get::<i64, _>("received_at")),
    }
}

/// All spans of one trace, ordered by start time. Org-scoped. NotFound if none.
pub async fn get_trace_spans(
    pool: &MySqlPool,
    trace_id: &str,
    org_id: OrgId,
) -> DbResult<Vec<Span>> {
    let rows = sqlx::query(
        "SELECT span_id, trace_id, parent_span_id, service_name, name, kind, start_ns, end_ns,
                duration_ms, status_code, status_message, attributes, received_at
         FROM spans WHERE trace_id = ? AND org_id = ? ORDER BY start_ns",
    )
    .bind(trace_id)
    .bind(org_id.0.to_string())
    .fetch_all(pool)
    .await?;
    if rows.is_empty() {
        return Err(DbError::NotFound);
    }
    Ok(rows.iter().map(span_from).collect())
}

/// Per-trace aggregate built in Rust (no LATERAL/ARRAY_AGG).
#[derive(Default)]
struct Agg {
    min_start: i64,
    max_end: i64,
    count: i64,
    errors: i64,
    services: BTreeSet<String>,
    min_recv: i64,
    root: Option<(i64, String, String)>, // (start_ns, service, name), parent IS NULL
}

/// Recent traces, newest first, filtered. Fetches the org's spans (capped) and
/// assembles one summary per trace_id in Rust — mirrors the PG aggregation +
/// HAVING filters + (received_at, trace_id) keyset.
pub async fn list_traces(
    pool: &MySqlPool,
    f: TraceFilter<'_>,
    org_id: OrgId,
) -> DbResult<Vec<TraceSummary>> {
    let limit = f.limit.clamp(1, 500) as usize;
    let rows = sqlx::query(
        "SELECT trace_id, parent_span_id, service_name, name, start_ns, end_ns, status_code,
                received_at
         FROM spans WHERE org_id = ? ORDER BY received_at DESC LIMIT 100000",
    )
    .bind(org_id.0.to_string())
    .fetch_all(pool)
    .await?;

    let mut by: BTreeMap<String, Agg> = BTreeMap::new();
    for r in &rows {
        let tid = r.get::<String, _>("trace_id");
        let start = r.get::<i64, _>("start_ns");
        let end = r.get::<i64, _>("end_ns");
        let recv = r.get::<i64, _>("received_at");
        let svc = r.get::<String, _>("service_name");
        let status = r.get::<i16, _>("status_code");
        let a = by.entry(tid).or_insert_with(|| Agg {
            min_start: start,
            max_end: end,
            min_recv: recv,
            ..Default::default()
        });
        a.min_start = a.min_start.min(start);
        a.max_end = a.max_end.max(end);
        a.min_recv = a.min_recv.min(recv);
        a.count += 1;
        if status == 2 {
            a.errors += 1;
        }
        a.services.insert(svc.clone());
        if r.get::<Option<String>, _>("parent_span_id").is_none() {
            let nm = r.get::<String, _>("name");
            match &a.root {
                Some((rs, _, _)) if *rs <= start => {}
                _ => a.root = Some((start, svc, nm)),
            }
        }
    }

    // Resolve the keyset cursor row's started_at (may be outside the cap).
    let before = match f.before_id {
        Some(bid) => {
            let recv: Option<i64> = sqlx::query_scalar(
                "SELECT MIN(received_at) FROM spans WHERE trace_id = ? AND org_id = ?",
            )
            .bind(bid)
            .bind(org_id.0.to_string())
            .fetch_one(pool)
            .await?;
            recv.map(|r| (r, bid.to_string()))
        }
        None => None,
    };

    let ql = f.q.map(|q| q.to_lowercase());
    let mut out: Vec<TraceSummary> = by
        .into_iter()
        .map(|(trace_id, a)| {
            let (root_service, root_name) = a
                .root
                .map(|(_, s, n)| (s, n))
                .unwrap_or_else(|| ("unknown".to_string(), String::new()));
            TraceSummary {
                trace_id,
                root_service,
                root_name,
                start_ns: a.min_start,
                duration_ms: (a.max_end - a.min_start) as f64 / 1_000_000.0,
                span_count: a.count,
                error_count: a.errors,
                services: a.services.into_iter().collect(),
                started_at: ts(a.min_recv),
            }
        })
        .filter(|t| f.service.is_none_or(|s| t.services.iter().any(|x| x == s)))
        .filter(|t| f.min_duration_ms.is_none_or(|d| t.duration_ms >= d))
        .filter(|t| !f.errors_only || t.error_count > 0)
        .filter(|t| {
            ql.as_ref().is_none_or(|q| {
                t.root_name.to_lowercase().contains(q)
                    || t.root_service.to_lowercase().contains(q)
                    || t.trace_id.to_lowercase().contains(q)
            })
        })
        .filter(|t| {
            // keyset: strictly older than the cursor by (received_at, trace_id) desc.
            before.as_ref().is_none_or(|(brecv, bid)| {
                let trecv = t.started_at.unix_timestamp();
                trecv < *brecv || (trecv == *brecv && t.trace_id.as_str() < bid.as_str())
            })
        })
        .collect();

    out.sort_by(|a, b| {
        b.started_at
            .cmp(&a.started_at)
            .then(b.trace_id.cmp(&a.trace_id))
    });
    out.truncate(limit);
    Ok(out)
}

/// Service dependency edges (caller→callee) over the window: calls, errors, p95
/// callee latency. Cross-service parent/child pairs, aggregated in Rust.
pub async fn service_map(
    pool: &MySqlPool,
    window_hours: i64,
    org_id: OrgId,
) -> DbResult<Vec<ServiceEdge>> {
    let cutoff = now_unix() - window_hours.clamp(1, 720) * 3600;
    let rows = sqlx::query(
        "SELECT parent.service_name AS from_s, child.service_name AS to_s,
                child.status_code AS sc, child.duration_ms AS dur
         FROM spans child JOIN spans parent
              ON child.parent_span_id = parent.span_id AND parent.org_id = ?
         WHERE child.service_name <> parent.service_name
           AND child.received_at > ? AND child.org_id = ?",
    )
    .bind(org_id.0.to_string())
    .bind(cutoff)
    .bind(org_id.0.to_string())
    .fetch_all(pool)
    .await?;

    struct E {
        calls: i64,
        errors: i64,
        durs: Vec<f64>,
    }
    let mut by: BTreeMap<(String, String), E> = BTreeMap::new();
    for r in &rows {
        let e = by
            .entry((r.get::<String, _>("from_s"), r.get::<String, _>("to_s")))
            .or_insert(E {
                calls: 0,
                errors: 0,
                durs: Vec::new(),
            });
        e.calls += 1;
        if r.get::<i16, _>("sc") == 2 {
            e.errors += 1;
        }
        e.durs.push(r.get::<f64, _>("dur"));
    }
    let mut out: Vec<ServiceEdge> = by
        .into_iter()
        .map(|((from_service, to_service), mut e)| ServiceEdge {
            from_service,
            to_service,
            calls: e.calls,
            errors: e.errors,
            p95_ms: p_cont(&mut e.durs, 0.95),
        })
        .collect();
    out.sort_by_key(|x| std::cmp::Reverse(x.calls));
    out.truncate(300);
    Ok(out)
}

/// Per-(service, operation) APM rollup over the window. Percentiles in Rust.
pub async fn operation_stats(
    pool: &MySqlPool,
    service: &str,
    window_hours: i64,
    org_id: OrgId,
) -> DbResult<Vec<OperationStat>> {
    let cutoff = now_unix() - window_hours.clamp(1, 720) * 3600;
    let rows = sqlx::query(
        "SELECT service_name, name, status_code, duration_ms FROM spans
         WHERE received_at > ? AND (? = '' OR service_name = ?) AND org_id = ?",
    )
    .bind(cutoff)
    .bind(service)
    .bind(service)
    .bind(org_id.0.to_string())
    .fetch_all(pool)
    .await?;

    struct S {
        calls: i64,
        errors: i64,
        durs: Vec<f64>,
    }
    let mut by: BTreeMap<(String, String), S> = BTreeMap::new();
    for r in &rows {
        let s = by
            .entry((
                r.get::<String, _>("service_name"),
                r.get::<String, _>("name"),
            ))
            .or_insert(S {
                calls: 0,
                errors: 0,
                durs: Vec::new(),
            });
        s.calls += 1;
        if r.get::<i16, _>("status_code") == 2 {
            s.errors += 1;
        }
        s.durs.push(r.get::<f64, _>("duration_ms"));
    }
    let mut out: Vec<OperationStat> = by
        .into_iter()
        .map(|((service, operation), mut s)| {
            let max_ms = s.durs.iter().cloned().fold(0.0_f64, f64::max);
            let avg_ms = if s.durs.is_empty() {
                0.0
            } else {
                s.durs.iter().sum::<f64>() / s.durs.len() as f64
            };
            OperationStat {
                service,
                operation,
                calls: s.calls,
                errors: s.errors,
                error_rate: if s.calls > 0 {
                    100.0 * s.errors as f64 / s.calls as f64
                } else {
                    0.0
                },
                p50_ms: p_cont(&mut s.durs, 0.50).unwrap_or(0.0),
                p95_ms: p_cont(&mut s.durs, 0.95).unwrap_or(0.0),
                p99_ms: p_cont(&mut s.durs, 0.99).unwrap_or(0.0),
                avg_ms,
                max_ms,
            }
        })
        .collect();
    out.sort_by_key(|x| std::cmp::Reverse(x.calls));
    out.truncate(500);
    Ok(out)
}

/// p95 latency trend for one (service, operation), bucketed oldest→newest.
pub async fn operation_trend(
    pool: &MySqlPool,
    service: &str,
    operation: &str,
    window_hours: i64,
    buckets: i64,
    org_id: OrgId,
) -> DbResult<Vec<f64>> {
    let hours = window_hours.clamp(1, 720);
    let buckets = buckets.clamp(2, 200);
    let step = ((hours * 3600) / buckets).max(1);
    let origin = now_unix() - hours * 3600;
    let sql = format!(
        "SELECT (received_at - {origin}) DIV {step} AS bucket, duration_ms FROM spans
         WHERE service_name = ? AND name = ? AND received_at > {origin} AND org_id = ?
         ORDER BY bucket"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(service)
        .bind(operation)
        .bind(org_id.0.to_string())
        .fetch_all(pool)
        .await?;
    let mut by: BTreeMap<i64, Vec<f64>> = BTreeMap::new();
    for r in &rows {
        by.entry(r.get::<i64, _>("bucket"))
            .or_default()
            .push(r.get::<f64, _>("duration_ms"));
    }
    Ok(by
        .into_values()
        .filter_map(|mut v| p_cont(&mut v, 0.95))
        .collect())
}

/// Delete spans older than `days`. Returns rows removed.
pub async fn prune(pool: &MySqlPool, days: i32) -> DbResult<u64> {
    let cutoff = now_unix() - days.max(0) as i64 * 86400;
    super::chunked_delete_older(pool, "spans", "received_at", cutoff).await
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    fn span(
        span_id: &str,
        trace: &str,
        parent: Option<&str>,
        svc: &str,
        name: &str,
        dur_ms: f64,
        status: i16,
    ) -> ParsedSpan {
        ParsedSpan {
            trace_id: trace.into(),
            span_id: span_id.into(),
            parent_span_id: parent.map(|p| p.into()),
            service_name: svc.into(),
            name: name.into(),
            kind: 2,
            start_ns: 1_000_000,
            end_ns: 1_000_000 + (dur_ms * 1_000_000.0) as i64,
            status_code: status,
            status_message: None,
            attributes: serde_json::json!({}),
        }
    }

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn insert_assemble_servicemap_opstats(pool: MySqlPool) {
        let org = super::super::oid(DEF);
        // trace t1: api(root) → db(child, error). trace t2: api(root) ok.
        let n = insert_spans(
            &pool,
            &[
                span("s1", "t1", None, "api", "GET /x", 50.0, 0),
                span("s2", "t1", Some("s1"), "db", "SELECT", 30.0, 2),
                span("s3", "t2", None, "api", "GET /y", 10.0, 0),
            ],
            org,
        )
        .await
        .unwrap();
        assert_eq!(n, 3);
        // dedup: re-insert s1 → 0 new.
        assert_eq!(
            insert_spans(
                &pool,
                &[span("s1", "t1", None, "api", "GET /x", 50.0, 0)],
                org
            )
            .await
            .unwrap(),
            0
        );

        // get_trace_spans (waterfall).
        let t1 = get_trace_spans(&pool, "t1", org).await.unwrap();
        assert_eq!(t1.len(), 2);
        assert!(matches!(
            get_trace_spans(&pool, "nope", org).await,
            Err(DbError::NotFound)
        ));

        // list_traces: 2 traces; errors_only → only t1.
        let all = list_traces(
            &pool,
            TraceFilter {
                limit: 100,
                ..Default::default()
            },
            org,
        )
        .await
        .unwrap();
        assert_eq!(all.len(), 2);
        let errs = list_traces(
            &pool,
            TraceFilter {
                errors_only: true,
                limit: 100,
                ..Default::default()
            },
            org,
        )
        .await
        .unwrap();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].trace_id, "t1");
        assert_eq!(errs[0].root_service, "api");
        assert_eq!(errs[0].error_count, 1);
        assert_eq!(errs[0].span_count, 2);

        // q on root name.
        let q = list_traces(
            &pool,
            TraceFilter {
                q: Some("get /y"),
                limit: 100,
                ..Default::default()
            },
            org,
        )
        .await
        .unwrap();
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].trace_id, "t2");

        // service_map: api → db edge, 1 call, 1 error, p95 = 30.
        let edges = service_map(&pool, 24, org).await.unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].from_service, "api");
        assert_eq!(edges[0].to_service, "db");
        assert_eq!(edges[0].calls, 1);
        assert_eq!(edges[0].errors, 1);
        assert_eq!(edges[0].p95_ms, Some(30.0));

        // operation_stats: db error_rate 100%, p95 = 30.
        let stats = operation_stats(&pool, "", 24, org).await.unwrap();
        let db = stats.iter().find(|s| s.service == "db").unwrap();
        assert_eq!(db.calls, 1);
        assert_eq!(db.errors, 1);
        assert!((db.error_rate - 100.0).abs() < 1e-9);
        assert_eq!(db.p95_ms, 30.0);

        // operation_trend: api GET /x has a p95 point.
        let trend = operation_trend(&pool, "api", "GET /x", 24, 24, org)
            .await
            .unwrap();
        assert_eq!(trend.len(), 1);
        assert_eq!(trend[0], 50.0);

        // prune: backdate t2's span 10d, prune(7) removes it.
        sqlx::query(
            "UPDATE spans SET received_at = UNIX_TIMESTAMP() - 10*86400 WHERE span_id = 's3'",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(prune(&pool, 7).await.unwrap(), 1);
        assert_eq!(
            list_traces(
                &pool,
                TraceFilter {
                    limit: 100,
                    ..Default::default()
                },
                org
            )
            .await
            .unwrap()
            .len(),
            1
        );
    }
}
