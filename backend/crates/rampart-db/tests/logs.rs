//! Integration tests for log read aggregates (`rampart_db::logs`).
//! Covers the volume histogram (total + error split, filtered) and level counts.

use rampart_core::log::ParsedLog;
use rampart_db::logs::{self, LogFilter};
use sqlx::PgPool;

fn line(severity: i16, service: &str, body: &str) -> ParsedLog {
    ParsedLog {
        time_ns: 0, // server-stamps now()
        severity,
        severity_text: None,
        service_name: service.into(),
        body: body.into(),
        trace_id: None,
        span_id: None,
        attributes: serde_json::json!({}),
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn histogram_totals_and_error_split(pool: PgPool) {
    logs::insert_logs(
        &pool,
        &[
            line(9, "api", "ok one"),       // info
            line(13, "api", "warn two"),    // warn
            line(17, "api", "boom three"),  // error (>=17)
            line(21, "auth", "fatal four"), // fatal (>=17)
        ],
    )
    .await
    .unwrap();

    // All four land in the recent window; the aggregate sums total + errors.
    let buckets = logs::histogram(&pool, None, None, None, 24, 48).await.unwrap();
    let total: i64 = buckets.iter().map(|b| b.total).sum();
    let errors: i64 = buckets.iter().map(|b| b.errors).sum();
    assert_eq!(total, 4);
    assert_eq!(errors, 2, "severity >= 17 counts as error");

    // Service filter narrows to one service.
    let auth = logs::histogram(&pool, Some("auth"), None, None, 24, 48).await.unwrap();
    assert_eq!(auth.iter().map(|b| b.total).sum::<i64>(), 1);

    // Min-severity filter (error+) drops the info/warn lines.
    let errs_only = logs::histogram(&pool, None, Some(17), None, 24, 48).await.unwrap();
    assert_eq!(errs_only.iter().map(|b| b.total).sum::<i64>(), 2);

    // Full-text filter on the body.
    let boom = logs::histogram(&pool, None, None, Some("boom"), 24, 48).await.unwrap();
    assert_eq!(boom.iter().map(|b| b.total).sum::<i64>(), 1);
}

#[sqlx::test(migrations = "../../migrations")]
async fn level_counts_and_query_roundtrip(pool: PgPool) {
    logs::insert_logs(
        &pool,
        &[line(9, "api", "a"), line(17, "api", "b"), line(17, "web", "c")],
    )
    .await
    .unwrap();

    let counts = logs::level_counts(&pool, None, 24).await.unwrap();
    let err = counts.iter().find(|(lvl, _)| lvl == "error").map(|(_, n)| *n);
    assert_eq!(err, Some(2));

    // The plain query honours the service filter.
    let rows = logs::query_logs(
        &pool,
        LogFilter { service: Some("web"), min_severity: None, query: None, trace_id: None, span_id: None, limit: 50 },
    )
    .await
    .unwrap();
    assert_eq!(rows.len(), 1);
}
