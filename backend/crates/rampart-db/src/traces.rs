//! Trace storage (migration 0078).
//!
//! Spans are stored individually; a trace is assembled on read by `trace_id`.
//! [`insert_spans`] is the ingest hot path (dedup on span_id); the read side
//! lists traces (aggregate per trace_id), fetches a trace's spans for the
//! waterfall, and derives the service-dependency map from cross-service
//! parent/child pairs. Spans age out via [`prune`].

use crate::{DbError, DbPool, DbResult};
use rampart_core::trace::{ParsedSpan, ServiceEdge, Span, TraceSummary};
use time::OffsetDateTime;

/// Insert a batch of spans, de-duplicating on `span_id` (exporters retransmit).
/// Returns the number of new rows. Runs in one transaction.
pub async fn insert_spans(pool: &DbPool, spans: &[ParsedSpan]) -> DbResult<u64> {
    if spans.is_empty() {
        return Ok(0);
    }
    let mut tx = pool.begin().await?;
    let mut inserted = 0u64;
    for s in spans {
        let res = sqlx::query!(
            r#"
            INSERT INTO spans
                (span_id, trace_id, parent_span_id, service_name, name, kind,
                 start_ns, end_ns, duration_ms, status_code, status_message, attributes)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            ON CONFLICT (span_id) DO NOTHING
            "#,
            s.span_id,
            s.trace_id,
            s.parent_span_id,
            s.service_name,
            s.name,
            s.kind,
            s.start_ns,
            s.end_ns,
            s.duration_ms(),
            s.status_code,
            s.status_message,
            s.attributes,
        )
        .execute(&mut *tx)
        .await?;
        inserted += res.rows_affected();
    }
    tx.commit().await?;
    Ok(inserted)
}

struct SummaryRow {
    trace_id: String,
    root_service: Option<String>,
    root_name: Option<String>,
    start_ns: Option<i64>,
    duration_ms: Option<f64>,
    span_count: Option<i64>,
    error_count: Option<i64>,
    services: Option<Vec<String>>,
    started_at: Option<OffsetDateTime>,
}

impl From<SummaryRow> for TraceSummary {
    fn from(r: SummaryRow) -> Self {
        TraceSummary {
            trace_id: r.trace_id,
            root_service: r.root_service.unwrap_or_else(|| "unknown".to_string()),
            root_name: r.root_name.unwrap_or_default(),
            start_ns: r.start_ns.unwrap_or(0),
            duration_ms: r.duration_ms.unwrap_or(0.0),
            span_count: r.span_count.unwrap_or(0),
            error_count: r.error_count.unwrap_or(0),
            services: r.services.unwrap_or_default(),
            started_at: r.started_at.unwrap_or(OffsetDateTime::UNIX_EPOCH),
        }
    }
}

/// Filters for the trace list. All optional; `errors_only` false = no filter.
#[derive(Default)]
pub struct TraceFilter<'a> {
    /// Trace involves this service (any span).
    pub service: Option<&'a str>,
    /// Minimum total trace duration in ms.
    pub min_duration_ms: Option<f64>,
    /// Only traces with at least one error span.
    pub errors_only: bool,
    /// Substring on root operation / root service / trace_id.
    pub q: Option<&'a str>,
    pub limit: i64,
}

/// Recent traces, newest first, filtered. One row per trace_id with its root
/// span's service/operation, total duration, and error count. Filters apply
/// post-aggregation (HAVING) so a service/duration/error/text query narrows the
/// list without losing any of a matched trace's spans.
pub async fn list_traces(pool: &DbPool, f: TraceFilter<'_>) -> DbResult<Vec<TraceSummary>> {
    let rows = sqlx::query_as!(
        SummaryRow,
        r#"
        SELECT
            s.trace_id AS "trace_id!",
            r.service_name AS "root_service",
            r.name AS "root_name",
            MIN(s.start_ns) AS "start_ns",
            (MAX(s.end_ns) - MIN(s.start_ns))::float8 / 1000000.0 AS "duration_ms",
            COUNT(*) AS "span_count",
            COUNT(*) FILTER (WHERE s.status_code = 2) AS "error_count",
            ARRAY_AGG(DISTINCT s.service_name) AS "services",
            MIN(s.received_at) AS "started_at"
        FROM spans s
        LEFT JOIN LATERAL (
            SELECT service_name, name FROM spans rr
            WHERE rr.trace_id = s.trace_id AND rr.parent_span_id IS NULL
            ORDER BY rr.start_ns LIMIT 1
        ) r ON true
        GROUP BY s.trace_id, r.service_name, r.name
        HAVING ($2::text IS NULL OR bool_or(s.service_name = $2))
           AND ($3::float8 IS NULL
                OR (MAX(s.end_ns) - MIN(s.start_ns))::float8 / 1000000.0 >= $3)
           AND (NOT $4 OR COUNT(*) FILTER (WHERE s.status_code = 2) > 0)
           AND ($5::text IS NULL
                OR r.name ILIKE '%' || $5 || '%'
                OR r.service_name ILIKE '%' || $5 || '%'
                OR s.trace_id ILIKE '%' || $5 || '%')
        ORDER BY MIN(s.received_at) DESC
        LIMIT $1
        "#,
        f.limit.clamp(1, 500),
        f.service,
        f.min_duration_ms,
        f.errors_only,
        f.q,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

struct SpanRow {
    span_id: String,
    trace_id: String,
    parent_span_id: Option<String>,
    service_name: String,
    name: String,
    kind: i16,
    start_ns: i64,
    end_ns: i64,
    duration_ms: f64,
    status_code: i16,
    status_message: Option<String>,
    attributes: Option<serde_json::Value>,
    received_at: OffsetDateTime,
}

impl From<SpanRow> for Span {
    fn from(r: SpanRow) -> Self {
        Span {
            span_id: r.span_id,
            trace_id: r.trace_id,
            parent_span_id: r.parent_span_id,
            service_name: r.service_name,
            name: r.name,
            kind: r.kind,
            start_ns: r.start_ns,
            end_ns: r.end_ns,
            duration_ms: r.duration_ms,
            status_code: r.status_code,
            status_message: r.status_message,
            attributes: r.attributes,
            received_at: r.received_at,
        }
    }
}

/// All spans of one trace, ordered by start time (for the waterfall).
pub async fn get_trace_spans(pool: &DbPool, trace_id: &str) -> DbResult<Vec<Span>> {
    let rows = sqlx::query_as!(
        SpanRow,
        r#"
        SELECT span_id, trace_id, parent_span_id, service_name, name, kind,
               start_ns, end_ns, duration_ms, status_code, status_message,
               attributes, received_at
        FROM spans
        WHERE trace_id = $1
        ORDER BY start_ns
        "#,
        trace_id,
    )
    .fetch_all(pool)
    .await?;
    if rows.is_empty() {
        return Err(DbError::NotFound);
    }
    Ok(rows.into_iter().map(Into::into).collect())
}

struct EdgeRow {
    from_service: Option<String>,
    to_service: Option<String>,
    calls: Option<i64>,
}

/// Service dependency edges (caller → callee) from cross-service parent/child
/// span pairs seen in the trailing window, with call counts.
pub async fn service_map(pool: &DbPool, window_hours: i64) -> DbResult<Vec<ServiceEdge>> {
    let rows = sqlx::query_as!(
        EdgeRow,
        r#"
        SELECT parent.service_name AS "from_service",
               child.service_name  AS "to_service",
               COUNT(*)            AS "calls"
        FROM spans child
        JOIN spans parent ON child.parent_span_id = parent.span_id
        WHERE child.service_name <> parent.service_name
          AND child.received_at > now() - make_interval(hours => $1)
        GROUP BY parent.service_name, child.service_name
        ORDER BY COUNT(*) DESC
        LIMIT 300
        "#,
        window_hours.clamp(1, 720) as i32,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| ServiceEdge {
            from_service: r.from_service.unwrap_or_default(),
            to_service: r.to_service.unwrap_or_default(),
            calls: r.calls.unwrap_or(0),
        })
        .collect())
}

/// Per-(service, operation) APM rollup over the last `window_hours`: call
/// volume, error count + rate, and p50/p95/p99/avg/max latency. The "services
/// & resources" table. `service` empty = all services.
pub async fn operation_stats(
    pool: &DbPool,
    service: &str,
    window_hours: i64,
) -> DbResult<Vec<rampart_core::OperationStat>> {
    struct Row {
        service: Option<String>,
        operation: Option<String>,
        calls: Option<i64>,
        errors: Option<i64>,
        p50: Option<f64>,
        p95: Option<f64>,
        p99: Option<f64>,
        avg_ms: Option<f64>,
        max_ms: Option<f64>,
    }
    let rows = sqlx::query_as!(
        Row,
        r#"
        SELECT service_name AS "service",
               name         AS "operation",
               COUNT(*)                                                     AS "calls",
               COUNT(*) FILTER (WHERE status_code = 2)                      AS "errors",
               percentile_cont(0.50) WITHIN GROUP (ORDER BY duration_ms)    AS "p50",
               percentile_cont(0.95) WITHIN GROUP (ORDER BY duration_ms)    AS "p95",
               percentile_cont(0.99) WITHIN GROUP (ORDER BY duration_ms)    AS "p99",
               AVG(duration_ms)                                             AS "avg_ms",
               MAX(duration_ms)                                             AS "max_ms"
        FROM spans
        WHERE received_at > now() - make_interval(hours => $1)
          AND ($2 = '' OR service_name = $2)
        GROUP BY service_name, name
        ORDER BY COUNT(*) DESC
        LIMIT 500
        "#,
        window_hours.clamp(1, 720) as i32,
        service,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            let calls = r.calls.unwrap_or(0);
            let errors = r.errors.unwrap_or(0);
            rampart_core::OperationStat {
                service: r.service.unwrap_or_default(),
                operation: r.operation.unwrap_or_default(),
                calls,
                errors,
                error_rate: if calls > 0 {
                    100.0 * errors as f64 / calls as f64
                } else {
                    0.0
                },
                p50_ms: r.p50.unwrap_or(0.0),
                p95_ms: r.p95.unwrap_or(0.0),
                p99_ms: r.p99.unwrap_or(0.0),
                avg_ms: r.avg_ms.unwrap_or(0.0),
                max_ms: r.max_ms.unwrap_or(0.0),
            }
        })
        .collect())
}

/// Delete spans older than `days`. Returns rows removed.
pub async fn prune(pool: &DbPool, days: i32) -> DbResult<u64> {
    let result = sqlx::query!(
        "DELETE FROM spans WHERE received_at < now() - make_interval(days => $1)",
        days,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
