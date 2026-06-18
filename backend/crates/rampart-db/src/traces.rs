//! Trace storage (migration 0078).
//!
//! Spans are stored individually; a trace is assembled on read by `trace_id`.
//! [`insert_spans`] is the ingest hot path (dedup on span_id); the read side
//! lists traces (aggregate per trace_id), fetches a trace's spans for the
//! waterfall, and derives the service-dependency map from cross-service
//! parent/child pairs. Spans age out via [`prune`].

use crate::{DbError, DbPool, DbResult};
use rampart_core::ids::OrgId;
use rampart_core::trace::{ParsedSpan, ServiceEdge, Span, TraceSummary};
use time::OffsetDateTime;

/// Insert a batch of spans, de-duplicating on `span_id` (exporters retransmit).
/// Returns the number of new rows. Runs in one transaction.
pub async fn insert_spans(
    pool: &DbPool,
    spans: &[ParsedSpan],
    org_id: rampart_core::ids::OrgId,
) -> DbResult<u64> {
    if spans.is_empty() {
        return Ok(0);
    }
    // Bulk INSERT via UNNEST (single round-trip) instead of a per-span loop.
    let n = spans.len();
    let mut span_id = Vec::with_capacity(n);
    let mut trace_id = Vec::with_capacity(n);
    let mut parent = Vec::with_capacity(n);
    let mut svc = Vec::with_capacity(n);
    let mut name = Vec::with_capacity(n);
    let mut kind = Vec::with_capacity(n);
    let mut start_ns = Vec::with_capacity(n);
    let mut end_ns = Vec::with_capacity(n);
    let mut dur = Vec::with_capacity(n);
    let mut status_code = Vec::with_capacity(n);
    let mut status_msg = Vec::with_capacity(n);
    let mut attrs = Vec::with_capacity(n);
    for s in spans {
        span_id.push(s.span_id.clone());
        trace_id.push(s.trace_id.clone());
        parent.push(s.parent_span_id.clone());
        svc.push(s.service_name.clone());
        name.push(s.name.clone());
        kind.push(s.kind);
        start_ns.push(s.start_ns);
        end_ns.push(s.end_ns);
        dur.push(s.duration_ms());
        status_code.push(s.status_code);
        status_msg.push(s.status_message.clone());
        attrs.push(s.attributes.clone());
    }
    let res = sqlx::query!(
        r#"
        INSERT INTO spans
            (span_id, trace_id, parent_span_id, service_name, name, kind,
             start_ns, end_ns, duration_ms, status_code, status_message, attributes, org_id)
        SELECT * FROM UNNEST(
            $1::text[], $2::text[], $3::text[], $4::text[], $5::text[], $6::int2[],
            $7::int8[], $8::int8[], $9::float8[], $10::int2[], $11::text[], $12::jsonb[],
            ARRAY_FILL($13::uuid, ARRAY[array_length($1::text[], 1)])
        )
        ON CONFLICT (span_id) DO NOTHING
        "#,
        &span_id,
        &trace_id,
        &parent as &[Option<String>],
        &svc,
        &name,
        &kind,
        &start_ns,
        &end_ns,
        &dur,
        &status_code,
        &status_msg as &[Option<String>],
        &attrs,
        org_id.0,
    )
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
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
    /// Keyset cursor for "load older": the last trace_id already shown. Returns
    /// traces strictly older than that trace's `(started_at, trace_id)`.
    pub before_id: Option<&'a str>,
    pub limit: i64,
}

/// Recent traces, newest first, filtered. One row per trace_id with its root
/// span's service/operation, total duration, and error count. Filters apply
/// post-aggregation (HAVING) so a service/duration/error/text query narrows the
/// list without losing any of a matched trace's spans.
pub async fn list_traces(
    pool: &DbPool,
    f: TraceFilter<'_>,
    org_id: OrgId,
) -> DbResult<Vec<TraceSummary>> {
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
              AND rr.org_id = $7
            ORDER BY rr.start_ns LIMIT 1
        ) r ON true
        WHERE s.org_id = $7
        GROUP BY s.trace_id, r.service_name, r.name
        HAVING ($2::text IS NULL OR bool_or(s.service_name = $2))
           AND ($3::float8 IS NULL
                OR (MAX(s.end_ns) - MIN(s.start_ns))::float8 / 1000000.0 >= $3)
           AND (NOT $4 OR COUNT(*) FILTER (WHERE s.status_code = 2) > 0)
           AND ($5::text IS NULL
                OR r.name ILIKE '%' || $5 || '%'
                OR r.service_name ILIKE '%' || $5 || '%'
                OR s.trace_id ILIKE '%' || $5 || '%')
           AND ($6::text IS NULL
                OR (MIN(s.received_at), s.trace_id)
                   < ((SELECT MIN(received_at) FROM spans WHERE trace_id = $6 AND org_id = $7), $6))
        ORDER BY MIN(s.received_at) DESC, s.trace_id DESC
        LIMIT $1
        "#,
        f.limit.clamp(1, 500),
        f.service,
        f.min_duration_ms,
        f.errors_only,
        f.q,
        f.before_id,
        org_id.0,
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

/// All spans of one trace, ordered by start time (for the waterfall). Scoped to
/// `org_id` — trace ids are no longer globally unique across tenants (Phase 5).
pub async fn get_trace_spans(pool: &DbPool, trace_id: &str, org_id: OrgId) -> DbResult<Vec<Span>> {
    let rows = sqlx::query_as!(
        SpanRow,
        r#"
        SELECT span_id, trace_id, parent_span_id, service_name, name, kind,
               start_ns, end_ns, duration_ms, status_code, status_message,
               attributes, received_at
        FROM spans
        WHERE trace_id = $1 AND org_id = $2
        ORDER BY start_ns
        "#,
        trace_id,
        org_id.0,
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
    errors: Option<i64>,
    p95_ms: Option<f64>,
}

/// Service dependency edges (caller → callee) from cross-service parent/child
/// span pairs seen in the trailing window — call count, error count, and p95
/// latency of the callee span, so the map can size + color edges by health.
pub async fn service_map(
    pool: &DbPool,
    window_hours: i64,
    org_id: OrgId,
) -> DbResult<Vec<ServiceEdge>> {
    let rows = sqlx::query_as!(
        EdgeRow,
        r#"
        SELECT parent.service_name AS "from_service",
               child.service_name  AS "to_service",
               COUNT(*)            AS "calls",
               COUNT(*) FILTER (WHERE child.status_code = 2) AS "errors",
               percentile_cont(0.95) WITHIN GROUP (ORDER BY child.duration_ms) AS "p95_ms"
        FROM spans child
        JOIN spans parent ON child.parent_span_id = parent.span_id
                         AND parent.org_id = $2
        WHERE child.service_name <> parent.service_name
          AND child.received_at > now() - make_interval(hours => $1)
          AND child.org_id = $2
        GROUP BY parent.service_name, child.service_name
        ORDER BY COUNT(*) DESC
        LIMIT 300
        "#,
        window_hours.clamp(1, 720) as i32,
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| ServiceEdge {
            from_service: r.from_service.unwrap_or_default(),
            to_service: r.to_service.unwrap_or_default(),
            calls: r.calls.unwrap_or(0),
            errors: r.errors.unwrap_or(0),
            p95_ms: r.p95_ms,
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
    org_id: OrgId,
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
          AND org_id = $3
        GROUP BY service_name, name
        ORDER BY COUNT(*) DESC
        LIMIT 500
        "#,
        window_hours.clamp(1, 720) as i32,
        service,
        org_id.0,
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

/// p95 latency trend for one (service, operation) over the window, bucketed
/// into ~`buckets` points (oldest → newest) for a sparkline. Empty buckets are
/// dropped.
pub async fn operation_trend(
    pool: &DbPool,
    service: &str,
    operation: &str,
    window_hours: i64,
    buckets: i64,
    org_id: OrgId,
) -> DbResult<Vec<f64>> {
    let hours = window_hours.clamp(1, 720);
    let buckets = buckets.clamp(2, 200);
    let step = ((hours * 3600) / buckets).max(1);
    let rows = sqlx::query!(
        r#"
        SELECT date_bin(make_interval(secs => $4), received_at,
                        now() - make_interval(hours => $3)) AS "bucket!",
               percentile_cont(0.95) WITHIN GROUP (ORDER BY duration_ms) AS "p95"
        FROM spans
        WHERE service_name = $1 AND name = $2
          AND received_at > now() - make_interval(hours => $3)
          AND org_id = $5
        GROUP BY 1 ORDER BY 1
        "#,
        service,
        operation,
        hours as i32,
        step as f64,
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().filter_map(|r| r.p95).collect())
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
