//! Log storage (migration 0079).
//!
//! [`insert_logs`] is the ingest hot path (one row per record, server-stamped
//! id). The read side is a filtered recent-logs query (service / min-severity /
//! body search / trace_id) plus a service list for the filter UI. Logs age out
//! via [`prune`]. Parsing lives in `rampart_core::log`.

use crate::{DbPool, DbResult};
use rampart_core::ids::OrgId;
use rampart_core::log::{coarse_level, ParsedLog};
use rampart_core::LogEntry;
use time::OffsetDateTime;
use uuid::Uuid;

/// Insert a batch of log records (one transaction). Returns rows written.
pub async fn insert_logs(
    pool: &DbPool,
    logs: &[ParsedLog],
    org_id: rampart_core::ids::OrgId,
) -> DbResult<u64> {
    if logs.is_empty() {
        return Ok(0);
    }
    // One bulk INSERT via UNNEST instead of a per-row loop — column-parallel
    // arrays expand into rows server-side, a single round-trip.
    let n = logs.len();
    let mut ids = Vec::with_capacity(n);
    let mut ts = Vec::with_capacity(n);
    let mut sev = Vec::with_capacity(n);
    let mut sevt = Vec::with_capacity(n);
    let mut svc = Vec::with_capacity(n);
    let mut body = Vec::with_capacity(n);
    let mut trace = Vec::with_capacity(n);
    let mut span = Vec::with_capacity(n);
    let mut attrs = Vec::with_capacity(n);
    for l in logs {
        ids.push(Uuid::now_v7());
        ts.push(if l.time_ns > 0 {
            OffsetDateTime::from_unix_timestamp_nanos(l.time_ns as i128)
                .unwrap_or_else(|_| OffsetDateTime::now_utc())
        } else {
            OffsetDateTime::now_utc()
        });
        sev.push(l.severity);
        sevt.push(l.severity_text.clone());
        svc.push(l.service_name.clone());
        body.push(l.body.clone());
        trace.push(l.trace_id.clone());
        span.push(l.span_id.clone());
        attrs.push(l.attributes.clone());
    }
    let res = sqlx::query!(
        r#"
        INSERT INTO logs
            (id, ts, severity, severity_text, service_name, body, trace_id, span_id, attributes, org_id)
        SELECT * FROM UNNEST(
            $1::uuid[], $2::timestamptz[], $3::int2[], $4::text[], $5::text[],
            $6::text[], $7::text[], $8::text[], $9::jsonb[],
            ARRAY_FILL($10::uuid, ARRAY[array_length($1::uuid[], 1)])
        )
        "#,
        &ids,
        &ts,
        &sev,
        &sevt as &[Option<String>],
        &svc,
        &body,
        &trace as &[Option<String>],
        &span as &[Option<String>],
        &attrs,
        org_id.0,
    )
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

struct LogRow {
    id: Uuid,
    ts: OffsetDateTime,
    severity: i16,
    severity_text: Option<String>,
    service_name: String,
    body: String,
    trace_id: Option<String>,
    span_id: Option<String>,
    attributes: Option<serde_json::Value>,
}

impl From<LogRow> for LogEntry {
    fn from(r: LogRow) -> Self {
        LogEntry {
            id: r.id,
            ts: r.ts,
            severity: r.severity,
            level: coarse_level(r.severity).to_string(),
            severity_text: r.severity_text,
            service_name: r.service_name,
            body: r.body,
            trace_id: r.trace_id,
            span_id: r.span_id,
            attributes: r.attributes,
        }
    }
}

/// Filters for [`query_logs`]. Any `None` is unconstrained.
#[derive(Default)]
pub struct LogFilter<'a> {
    pub service: Option<&'a str>,
    /// Minimum OTLP severity number (e.g. 13 for warn+).
    pub min_severity: Option<i16>,
    /// Full-text search over the body (Postgres `websearch_to_tsquery` —
    /// supports bare words, "quoted phrases", OR and -negation).
    pub query: Option<&'a str>,
    pub trace_id: Option<&'a str>,
    /// Narrow to a single span within a trace (the span→logs pivot).
    pub span_id: Option<&'a str>,
    /// Time window in hours (bounds `received_at`, matching the histogram +
    /// level-count reads). `None` = unbounded (used by the trace/span pivots).
    pub hours: Option<i32>,
    /// Keyset cursor for "load older": the id of the last row already shown.
    /// Returns rows strictly older than that row's `(ts, id)` — the backend
    /// resolves its timestamp, so no client-side precision loss. `None` = first
    /// page.
    pub before_id: Option<uuid::Uuid>,
    pub limit: i64,
}

/// Recent logs matching the filter, newest first. Scoped to `org_id` (the
/// `before_id` keyset row is resolved within the same org so a cross-org cursor
/// can't leak ordering).
pub async fn query_logs(pool: &DbPool, f: LogFilter<'_>, org_id: OrgId) -> DbResult<Vec<LogEntry>> {
    let rows = sqlx::query_as!(
        LogRow,
        r#"
        SELECT id, ts, severity, severity_text, service_name, body,
               trace_id, span_id, attributes
        FROM logs
        WHERE ($1::text IS NULL OR service_name = $1)
          AND ($2::int2 IS NULL OR severity >= $2)
          AND ($3::text IS NULL OR body_tsv @@ websearch_to_tsquery('english', $3))
          AND ($4::text IS NULL OR trace_id = $4)
          AND ($6::text IS NULL OR span_id = $6)
          AND ($7::int4 IS NULL OR received_at > now() - make_interval(hours => $7))
          AND ($8::uuid IS NULL
               OR (ts, id) < (SELECT ts, id FROM logs WHERE id = $8 AND org_id = $9))
          AND org_id = $9
        ORDER BY ts DESC, id DESC
        LIMIT $5
        "#,
        f.service,
        f.min_severity,
        f.query,
        f.trace_id,
        f.limit.clamp(1, 1000),
        f.span_id,
        f.hours.map(|h| h.clamp(1, 720)),
        f.before_id,
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

/// Count of log records per coarse level over the last `hours`, optionally
/// scoped to one service. Folds OTLP severity numbers into the coarse buckets
/// (trace/debug/info/warn/error/fatal) for an at-a-glance volume breakdown.
pub async fn level_counts(
    pool: &DbPool,
    service: Option<&str>,
    hours: i32,
    org_id: OrgId,
) -> DbResult<Vec<(String, i64)>> {
    let rows = sqlx::query!(
        r#"
        SELECT severity, COUNT(*) AS "count!"
        FROM logs
        WHERE received_at > now() - make_interval(hours => $1)
          AND ($2::text IS NULL OR service_name = $2)
          AND org_id = $3
        GROUP BY severity
        "#,
        hours.clamp(1, 720),
        service,
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    let mut acc: std::collections::BTreeMap<&'static str, i64> = std::collections::BTreeMap::new();
    for r in rows {
        *acc.entry(coarse_level(r.severity)).or_default() += r.count;
    }
    Ok(acc.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

/// One time bucket of the log-volume histogram: total + error-level counts.
#[derive(Debug, serde::Serialize)]
pub struct LogBucket {
    pub ts: time::OffsetDateTime,
    pub total: i64,
    pub errors: i64,
}

/// Time-bucketed log volume over the window, honouring the same service /
/// min-severity / full-text filters as the log query, with error-level (≥17)
/// counts split out so the UI can stack them. ~`buckets` points, oldest first.
pub async fn histogram(
    pool: &DbPool,
    service: Option<&str>,
    min_severity: Option<i16>,
    query: Option<&str>,
    hours: i32,
    buckets: i64,
    org_id: OrgId,
) -> DbResult<Vec<LogBucket>> {
    let hours = hours.clamp(1, 720);
    let buckets = buckets.clamp(2, 200);
    let step = ((hours as i64 * 3600) / buckets).max(1);
    let rows = sqlx::query!(
        r#"
        SELECT date_bin(make_interval(secs => $5), ts,
                        now() - make_interval(hours => $4)) AS "bucket!",
               COUNT(*)                              AS "total!",
               COUNT(*) FILTER (WHERE severity >= 17) AS "errors!"
        FROM logs
        WHERE received_at > now() - make_interval(hours => $4)
          AND ($1::text IS NULL OR service_name = $1)
          AND ($2::int2 IS NULL OR severity >= $2)
          AND ($3::text IS NULL OR body_tsv @@ websearch_to_tsquery('english', $3))
          AND org_id = $6
        GROUP BY 1 ORDER BY 1
        "#,
        service,
        min_severity,
        query,
        hours,
        step as f64,
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| LogBucket {
            ts: r.bucket,
            total: r.total,
            errors: r.errors,
        })
        .collect())
}

/// Distinct service names seen recently (for the filter dropdown).
pub async fn list_services(pool: &DbPool, org_id: OrgId) -> DbResult<Vec<String>> {
    let rows = sqlx::query_scalar!(
        r#"
        SELECT DISTINCT service_name AS "service_name!"
        FROM logs
        WHERE received_at > now() - make_interval(days => 7)
          AND org_id = $1
        ORDER BY service_name
        LIMIT 500
        "#,
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Delete logs older than `days`. Returns rows removed.
pub async fn prune(pool: &DbPool, days: i32) -> DbResult<u64> {
    // Chunked so a large backlog doesn't lock the high-volume logs table in one
    // multi-minute DELETE (see prune::batched_delete).
    crate::prune::batched_delete(pool, "logs", "received_at", days).await
}
