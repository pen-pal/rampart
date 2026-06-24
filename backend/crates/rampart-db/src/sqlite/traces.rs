//! SQLite `traces` domain — span storage + trace assembly (telemetry foundation
//! 2/2). Mirrors the PG free-fn surface: insert_spans / list_traces /
//! get_trace_spans / service_map / operation_stats / operation_trend / prune.
//!
//! SQLite has no `percentile_cont`, no `LATERAL`, no `ARRAY_AGG`, so the four
//! analytic reads (list_traces, service_map, operation_stats, operation_trend)
//! fetch the relevant span rows and aggregate in Rust — including a continuous
//! percentile (`p_cont`) matching PG's `percentile_cont`. insert/get/prune are
//! plain SQL. Dialect: hex ids TEXT, ns→INTEGER, double→REAL, jsonb attrs→TEXT,
//! received_at→INTEGER unix-seconds; `UNNEST … ON CONFLICT(span_id) DO NOTHING`
//! → per-row tx with the same conflict clause.

use super::ts;
use crate::traces::TraceFilter;
use crate::{DbError, DbResult};
use rampart_core::ids::OrgId;
use rampart_core::trace::{ParsedSpan, ServiceEdge, Span, TraceSummary};
use rampart_core::OperationStat;
use sqlx::{Row, SqlitePool};
use std::collections::BTreeMap;
use time::OffsetDateTime;

/// Continuous percentile (matches PG `percentile_cont`): rank = p·(n−1),
/// linear interpolation between the bracketing samples. `None` when empty.
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
pub async fn insert_spans(pool: &SqlitePool, spans: &[ParsedSpan], org_id: OrgId) -> DbResult<u64> {
    if spans.is_empty() {
        return Ok(0);
    }
    let org = org_id.0.to_string();
    let mut tx = pool.begin().await?;
    let mut n = 0u64;
    for batch in spans.chunks(super::insert_chunk(13)) {
        let mut qb = sqlx::QueryBuilder::new(
            "INSERT INTO spans \
                (span_id, trace_id, parent_span_id, service_name, name, kind, start_ns, end_ns, \
                 duration_ms, status_code, status_message, attributes, org_id) ",
        );
        qb.push_values(batch, |mut b, s| {
            b.push_bind(&s.span_id)
                .push_bind(&s.trace_id)
                .push_bind(&s.parent_span_id)
                .push_bind(&s.service_name)
                .push_bind(&s.name)
                .push_bind(s.kind)
                .push_bind(s.start_ns)
                .push_bind(s.end_ns)
                .push_bind(s.duration_ms())
                .push_bind(s.status_code)
                .push_bind(&s.status_message)
                .push_bind(serde_json::to_string(&s.attributes).unwrap_or_else(|_| "null".into()))
                .push_bind(&org);
        });
        qb.push(" ON CONFLICT(span_id) DO NOTHING");
        n += qb.build().execute(&mut *tx).await?.rows_affected();
    }
    tx.commit().await?;
    Ok(n)
}

fn span_from(r: &sqlx::sqlite::SqliteRow) -> Span {
    Span {
        span_id: r.get("span_id"),
        trace_id: r.get("trace_id"),
        parent_span_id: r.get("parent_span_id"),
        service_name: r.get("service_name"),
        name: r.get("name"),
        kind: r.get::<i64, _>("kind") as i16,
        start_ns: r.get("start_ns"),
        end_ns: r.get("end_ns"),
        duration_ms: r.get("duration_ms"),
        status_code: r.get::<i64, _>("status_code") as i16,
        status_message: r.get("status_message"),
        attributes: r
            .get::<Option<String>, _>("attributes")
            .and_then(|s| serde_json::from_str(&s).ok()),
        received_at: ts(r.get::<i64, _>("received_at")),
    }
}

/// All spans of one trace, ordered by start time. Org-scoped. NotFound if none.
pub async fn get_trace_spans(
    pool: &SqlitePool,
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

/// Recent traces, newest first, filtered — aggregated in SQL (`GROUP BY
/// trace_id`), not by scanning every span into Rust. Mirrors the PG free-fn:
/// per-trace rollup, the same HAVING filters, and a `(received_at, trace_id)`
/// keyset. The root span (service + name) is the earliest `parent_span_id IS
/// NULL` span, resolved with a correlated subquery (SQLite has no LATERAL);
/// `services` is `group_concat(DISTINCT …)` split on `,`. `spans_trace_idx`
/// backs the group + root lookups.
pub async fn list_traces(
    pool: &SqlitePool,
    f: TraceFilter<'_>,
    org_id: OrgId,
) -> DbResult<Vec<TraceSummary>> {
    let org = org_id.0.to_string();
    let limit = f.limit.clamp(1, 500);
    let errors_only = i64::from(f.errors_only);
    let rows = sqlx::query(
        "SELECT t.* FROM (
           SELECT
             s.trace_id AS trace_id,
             (SELECT rr.service_name FROM spans rr
                WHERE rr.trace_id = s.trace_id AND rr.parent_span_id IS NULL AND rr.org_id = ?
                ORDER BY rr.start_ns LIMIT 1) AS root_service,
             (SELECT rr.name FROM spans rr
                WHERE rr.trace_id = s.trace_id AND rr.parent_span_id IS NULL AND rr.org_id = ?
                ORDER BY rr.start_ns LIMIT 1) AS root_name,
             MIN(s.start_ns) AS start_ns,
             CAST(MAX(s.end_ns) - MIN(s.start_ns) AS REAL) / 1000000.0 AS duration_ms,
             COUNT(*) AS span_count,
             SUM(CASE WHEN s.status_code = 2 THEN 1 ELSE 0 END) AS error_count,
             group_concat(DISTINCT s.service_name) AS services,
             MIN(s.received_at) AS started_at
           FROM spans s
           WHERE s.org_id = ?
           GROUP BY s.trace_id
           HAVING (? IS NULL OR MAX(CASE WHEN s.service_name = ? THEN 1 ELSE 0 END) = 1)
              AND (? IS NULL OR CAST(MAX(s.end_ns) - MIN(s.start_ns) AS REAL) / 1000000.0 >= ?)
              AND (? = 0 OR SUM(CASE WHEN s.status_code = 2 THEN 1 ELSE 0 END) > 0)
         ) t
         WHERE (? IS NULL
                OR t.root_name LIKE '%' || ? || '%'
                OR t.root_service LIKE '%' || ? || '%'
                OR t.trace_id LIKE '%' || ? || '%')
           AND (? IS NULL
                OR t.started_at < (SELECT MIN(received_at) FROM spans WHERE trace_id = ? AND org_id = ?)
                OR (t.started_at = (SELECT MIN(received_at) FROM spans WHERE trace_id = ? AND org_id = ?)
                    AND t.trace_id < ?))
         ORDER BY t.started_at DESC, t.trace_id DESC
         LIMIT ?",
    )
    .bind(&org) // root_service correlated subquery
    .bind(&org) // root_name correlated subquery
    .bind(&org) // WHERE s.org_id
    .bind(f.service) // HAVING service IS NULL
    .bind(f.service) // HAVING service match
    .bind(f.min_duration_ms) // HAVING min_dur IS NULL
    .bind(f.min_duration_ms) // HAVING min_dur >=
    .bind(errors_only) // HAVING errors_only
    .bind(f.q) // outer q IS NULL
    .bind(f.q) // root_name LIKE
    .bind(f.q) // root_service LIKE
    .bind(f.q) // trace_id LIKE
    .bind(f.before_id) // keyset before IS NULL
    .bind(f.before_id) // cursor subquery (< branch)
    .bind(&org) // cursor subquery org (< branch)
    .bind(f.before_id) // cursor subquery (= branch)
    .bind(&org) // cursor subquery org (= branch)
    .bind(f.before_id) // trace_id < cursor
    .bind(limit) // LIMIT
    .fetch_all(pool)
    .await?;

    Ok(rows
        .iter()
        .map(|r| {
            let services = r
                .get::<Option<String>, _>("services")
                .unwrap_or_default()
                .split(',')
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
            TraceSummary {
                trace_id: r.get("trace_id"),
                root_service: r
                    .get::<Option<String>, _>("root_service")
                    .unwrap_or_else(|| "unknown".to_string()),
                root_name: r.get::<Option<String>, _>("root_name").unwrap_or_default(),
                start_ns: r.get("start_ns"),
                duration_ms: r.get("duration_ms"),
                span_count: r.get("span_count"),
                error_count: r.get("error_count"),
                services,
                started_at: ts(r.get::<i64, _>("started_at")),
            }
        })
        .collect())
}

/// Service dependency edges (caller→callee) over the window: calls, errors, p95
/// callee latency. Cross-service parent/child pairs, aggregated in Rust.
pub async fn service_map(
    pool: &SqlitePool,
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
        if r.get::<i64, _>("sc") == 2 {
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
    pool: &SqlitePool,
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
        if r.get::<i64, _>("status_code") == 2 {
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
    pool: &SqlitePool,
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
        "SELECT (received_at - {origin}) / {step} AS bucket, duration_ms FROM spans
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
pub async fn prune(pool: &SqlitePool, days: i32) -> DbResult<u64> {
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

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn insert_assemble_servicemap_opstats(pool: SqlitePool) {
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

        // service filter (q on root name).
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

        // operation_stats: api has 2 ops, db has 1; db error_rate 100%.
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
        sqlx::query("UPDATE spans SET received_at = unixepoch() - 10*86400 WHERE span_id = 's3'")
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

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn list_traces_keyset_paginates(pool: SqlitePool) {
        let org = super::super::oid(DEF);
        insert_spans(
            &pool,
            &[
                span("a1", "ta", None, "api", "GET /a", 10.0, 0),
                span("b1", "tb", None, "api", "GET /b", 10.0, 0),
                span("c1", "tc", None, "api", "GET /c", 10.0, 0),
            ],
            org,
        )
        .await
        .unwrap();
        // Distinct received_at so the DESC order is deterministic: tc newest.
        for (id, recv) in [("a1", 100), ("b1", 200), ("c1", 300)] {
            sqlx::query("UPDATE spans SET received_at = ? WHERE span_id = ?")
                .bind(recv)
                .bind(id)
                .execute(&pool)
                .await
                .unwrap();
        }

        // Page 1: newest first, limit 2.
        let p1 = list_traces(
            &pool,
            TraceFilter {
                limit: 2,
                ..Default::default()
            },
            org,
        )
        .await
        .unwrap();
        assert_eq!(
            p1.iter().map(|t| t.trace_id.as_str()).collect::<Vec<_>>(),
            ["tc", "tb"]
        );

        // Page 2: strictly older than the tb cursor → ta only.
        let p2 = list_traces(
            &pool,
            TraceFilter {
                limit: 2,
                before_id: Some("tb"),
                ..Default::default()
            },
            org,
        )
        .await
        .unwrap();
        assert_eq!(
            p2.iter().map(|t| t.trace_id.as_str()).collect::<Vec<_>>(),
            ["ta"]
        );
    }
}
