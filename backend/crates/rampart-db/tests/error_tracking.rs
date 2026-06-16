//! Integration tests for the error-tracking read aggregates added this cycle:
//! the cross-project recent-open feed and the per-project event histogram.

use rampart_core::error_tracking::ParsedEvent;
use rampart_db::error_tracking as et;
use sqlx::PgPool;

fn event(exc_type: &str, message: &str) -> ParsedEvent {
    ParsedEvent {
        event_id: None,
        level: "error".into(),
        platform: None,
        environment: Some("production".into()),
        release: None,
        server_name: None,
        transaction: Some("GET /checkout".into()),
        message: Some(message.into()),
        exception_type: Some(exc_type.into()),
        exception_value: Some(message.into()),
        frames: vec![],
        fingerprint_override: None,
        raw: serde_json::json!({}),
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn recent_open_and_histogram(pool: PgPool) {
    let project = et::find_or_create_by_name(&pool, "[demo] web").await.unwrap();

    // Two distinct exceptions → two issues; the second exception twice → grouped.
    et::record_event(&pool, project.id, &event("TypeError", "x is undefined")).await.unwrap();
    et::record_event(&pool, project.id, &event("ValueError", "bad input")).await.unwrap();
    et::record_event(&pool, project.id, &event("ValueError", "bad input")).await.unwrap();

    // Two open issues across all projects, newest-seen first.
    let recent = et::recent_open_issues(&pool, 8).await.unwrap();
    assert_eq!(recent.len(), 2);
    assert!(recent.iter().all(|i| i.status == "unresolved"));

    // Histogram counts every event (3), not just issues.
    let hist = et::project_event_histogram(&pool, project.id, 168, 48).await.unwrap();
    assert_eq!(hist.iter().map(|b| b.count).sum::<i64>(), 3);

    // Resolving an issue drops it from the recent-open feed.
    et::set_issue_status(&pool, recent[0].id, "resolved").await.unwrap();
    assert_eq!(et::recent_open_issues(&pool, 8).await.unwrap().len(), 1);
}
