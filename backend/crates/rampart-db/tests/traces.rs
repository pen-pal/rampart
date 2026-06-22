//! Integration test for the enriched service map (`rampart_db::traces::service_map`):
//! per-edge call count, error count, and p95 latency of the callee span.

use rampart_core::trace::ParsedSpan;
use rampart_db::traces;
use sqlx::PgPool;

const TEST_ORG: rampart_core::ids::OrgId =
    rampart_core::ids::OrgId::from_uuid(rampart_core::org::DEFAULT_ORG_ID);

fn span(id: &str, parent: Option<&str>, service: &str, dur_ms: f64, err: bool) -> ParsedSpan {
    ParsedSpan {
        trace_id: "trace-1".into(),
        span_id: id.into(),
        parent_span_id: parent.map(Into::into),
        service_name: service.into(),
        name: format!("{service} op"),
        kind: 2,
        start_ns: 0,
        end_ns: (dur_ms * 1e6) as i64,
        status_code: if err { 2 } else { 0 },
        status_message: None,
        attributes: serde_json::json!({}),
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn service_map_edge_has_calls_errors_p95(pool: PgPool) {
    traces::insert_spans(
        &pool,
        &[
            span("p", None, "web", 300.0, false), // caller, same service as nothing downstream
            span("c1", Some("p"), "api", 100.0, false), // web -> api, ok
            span("c2", Some("p"), "api", 200.0, true), // web -> api, error
        ],
        TEST_ORG,
    )
    .await
    .unwrap();

    let edges = traces::service_map(&pool, 24, TEST_ORG).await.unwrap();
    let edge = edges
        .iter()
        .find(|e| e.from_service == "web" && e.to_service == "api")
        .expect("web -> api edge present");
    assert_eq!(edge.calls, 2);
    assert_eq!(edge.errors, 1);
    // p95 of {100, 200} ms sits in that range.
    let p95 = edge.p95_ms.expect("p95 computed");
    assert!((100.0..=200.0).contains(&p95), "p95 {p95} in range");

    // Same-service parent/child pairs are not edges (web has no self-edge here).
    assert!(!edges.iter().any(|e| e.from_service == e.to_service));
}

#[sqlx::test(migrations = "../../migrations")]
async fn operation_trend_returns_p95_series(pool: PgPool) {
    // Several spans for one (service, operation) → at least one bucket of p95.
    traces::insert_spans(
        &pool,
        &[
            span("a1", None, "api", 100.0, false),
            span("a2", None, "api", 150.0, false),
            span("a3", None, "api", 200.0, false),
        ],
        TEST_ORG,
    )
    .await
    .unwrap();

    let trend = traces::operation_trend(&pool, "api", "api op", 24, 24, TEST_ORG)
        .await
        .unwrap();
    assert!(!trend.is_empty(), "a recent bucket with p95");
    // p95 of {100,150,200} sits in range.
    assert!(trend.iter().all(|&p| (100.0..=200.0).contains(&p)));

    // A different operation has no data.
    assert!(
        traces::operation_trend(&pool, "api", "missing", 24, 24, TEST_ORG)
            .await
            .unwrap()
            .is_empty()
    );
}
