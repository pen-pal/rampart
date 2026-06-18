//! Integration tests for log read aggregates (`rampart_db::logs`).
//! Covers the volume histogram (total + error split, filtered) and level counts.

use rampart_core::log::ParsedLog;
use rampart_db::logs::{self, LogFilter};
use sqlx::PgPool;

const TEST_ORG: rampart_core::ids::OrgId =
    rampart_core::ids::OrgId::from_uuid(rampart_core::org::DEFAULT_ORG_ID);

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
        TEST_ORG,
    )
    .await
    .unwrap();

    // All four land in the recent window; the aggregate sums total + errors.
    let buckets = logs::histogram(&pool, None, None, None, 24, 48, TEST_ORG).await.unwrap();
    let total: i64 = buckets.iter().map(|b| b.total).sum();
    let errors: i64 = buckets.iter().map(|b| b.errors).sum();
    assert_eq!(total, 4);
    assert_eq!(errors, 2, "severity >= 17 counts as error");

    // Service filter narrows to one service.
    let auth = logs::histogram(&pool, Some("auth"), None, None, 24, 48, TEST_ORG).await.unwrap();
    assert_eq!(auth.iter().map(|b| b.total).sum::<i64>(), 1);

    // Min-severity filter (error+) drops the info/warn lines.
    let errs_only = logs::histogram(&pool, None, Some(17), None, 24, 48, TEST_ORG).await.unwrap();
    assert_eq!(errs_only.iter().map(|b| b.total).sum::<i64>(), 2);

    // Full-text filter on the body.
    let boom = logs::histogram(&pool, None, None, Some("boom"), 24, 48, TEST_ORG).await.unwrap();
    assert_eq!(boom.iter().map(|b| b.total).sum::<i64>(), 1);
}

#[sqlx::test(migrations = "../../migrations")]
async fn level_counts_and_query_roundtrip(pool: PgPool) {
    logs::insert_logs(
        &pool,
        &[line(9, "api", "a"), line(17, "api", "b"), line(17, "web", "c")],
        TEST_ORG,
    )
    .await
    .unwrap();

    let counts = logs::level_counts(&pool, None, 24, TEST_ORG).await.unwrap();
    let err = counts.iter().find(|(lvl, _)| lvl == "error").map(|(_, n)| *n);
    assert_eq!(err, Some(2));

    // The plain query honours the service filter.
    let rows = logs::query_logs(
        &pool,
        LogFilter { service: Some("web"), min_severity: None, query: None, trace_id: None, span_id: None, hours: None, before_id: None, limit: 50 },
        TEST_ORG,
    )
    .await
    .unwrap();
    assert_eq!(rows.len(), 1);
}

#[sqlx::test(migrations = "../../migrations")]
async fn keyset_pagination_covers_all_without_overlap(pool: PgPool) {
    // Five logs, all stamped ~now() (ties on ts) — so the (ts, id) keyset's id
    // tiebreak is what makes paging stable.
    logs::insert_logs(
        &pool,
        &[
            line(9, "api", "one"),
            line(9, "api", "two"),
            line(9, "api", "three"),
            line(9, "api", "four"),
            line(9, "api", "five"),
        ],
        TEST_ORG,
    )
    .await
    .unwrap();

    let page = |before: Option<uuid::Uuid>| {
        let pool = pool.clone();
        async move {
            logs::query_logs(
                &pool,
                LogFilter {
                    service: Some("api"),
                    min_severity: None,
                    query: None,
                    trace_id: None,
                    span_id: None,
                    hours: None,
                    before_id: before,
                    limit: 2,
                },
                TEST_ORG,
            )
            .await
            .unwrap()
        }
    };

    // Walk the cursor in pages of 2 and collect every id.
    let mut seen = Vec::new();
    let mut cursor = None;
    loop {
        let rows = page(cursor).await;
        if rows.is_empty() {
            break;
        }
        assert!(rows.len() <= 2);
        cursor = Some(rows.last().unwrap().id);
        seen.extend(rows.into_iter().map(|r| r.id));
    }

    assert_eq!(seen.len(), 5, "every row paged exactly once");
    let unique: std::collections::HashSet<_> = seen.iter().collect();
    assert_eq!(unique.len(), 5, "no row appears on two pages");
}

/// A non-Default org for cross-tenant read-isolation assertions.
const OTHER_ORG_UUID: uuid::Uuid = uuid::Uuid::from_u128(0x0c0ffe);

async fn make_other_org(pool: &PgPool) -> rampart_core::ids::OrgId {
    sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1, 'other', 'Other')")
        .bind(OTHER_ORG_UUID)
        .execute(pool)
        .await
        .unwrap();
    rampart_core::ids::OrgId::from_uuid(OTHER_ORG_UUID)
}

/// Phase 5 read-scoping: telemetry inserted under org A is invisible to a read
/// scoped to Default, and vice-versa — across the list, the aggregates, and the
/// service picker.
#[sqlx::test(migrations = "../../migrations")]
async fn reads_are_isolated_per_org(pool: PgPool) {
    let other = make_other_org(&pool).await;

    // Default org: one info + one error line, service "alpha".
    logs::insert_logs(&pool, &[line(9, "alpha", "default ok"), line(17, "alpha", "default boom")], TEST_ORG)
        .await
        .unwrap();
    // Other org: two lines, service "beta".
    logs::insert_logs(&pool, &[line(9, "beta", "other one"), line(9, "beta", "other two")], other)
        .await
        .unwrap();

    let def_filter = || LogFilter {
        service: None,
        min_severity: None,
        query: None,
        trace_id: None,
        span_id: None,
        hours: None,
        before_id: None,
        limit: 50,
    };

    // query_logs sees only its own org's rows.
    let def_rows = logs::query_logs(&pool, def_filter(), TEST_ORG).await.unwrap();
    assert_eq!(def_rows.len(), 2, "Default sees only its 2 rows");
    assert!(def_rows.iter().all(|r| r.service_name == "alpha"));

    let other_rows = logs::query_logs(&pool, def_filter(), other).await.unwrap();
    assert_eq!(other_rows.len(), 2, "Other sees only its 2 rows");
    assert!(other_rows.iter().all(|r| r.service_name == "beta"));

    // level_counts is per-org (Default has the only error line).
    let def_counts = logs::level_counts(&pool, None, 24, TEST_ORG).await.unwrap();
    assert_eq!(
        def_counts.iter().find(|(l, _)| l == "error").map(|(_, n)| *n),
        Some(1)
    );
    let other_counts = logs::level_counts(&pool, None, 24, other).await.unwrap();
    assert!(other_counts.iter().all(|(l, _)| l != "error"), "Other has no error line");

    // histogram totals are per-org.
    let def_total: i64 = logs::histogram(&pool, None, None, None, 24, 48, TEST_ORG)
        .await
        .unwrap()
        .iter()
        .map(|b| b.total)
        .sum();
    assert_eq!(def_total, 2);
    let other_total: i64 = logs::histogram(&pool, None, None, None, 24, 48, other)
        .await
        .unwrap()
        .iter()
        .map(|b| b.total)
        .sum();
    assert_eq!(other_total, 2);

    // service picker is per-org.
    assert_eq!(logs::list_services(&pool, TEST_ORG).await.unwrap(), vec!["alpha".to_string()]);
    assert_eq!(logs::list_services(&pool, other).await.unwrap(), vec!["beta".to_string()]);
}
