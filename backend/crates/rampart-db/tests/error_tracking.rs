//! Integration tests for the error-tracking read aggregates added this cycle:
//! the cross-project recent-open feed and the per-project event histogram.

use rampart_core::error_tracking::ParsedEvent;
use rampart_core::ids::OrgId;
use rampart_core::org::DEFAULT_ORG_ID;
use rampart_db::error_tracking as et;
use sqlx::PgPool;

fn def_org() -> OrgId {
    OrgId::from_uuid(DEFAULT_ORG_ID)
}

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
    let project = et::find_or_create_by_name(&pool, "[demo] web", def_org())
        .await
        .unwrap();

    // Two distinct exceptions → two issues; the second exception twice → grouped.
    et::record_event(&pool, project.id, &event("TypeError", "x is undefined"))
        .await
        .unwrap();
    et::record_event(&pool, project.id, &event("ValueError", "bad input"))
        .await
        .unwrap();
    et::record_event(&pool, project.id, &event("ValueError", "bad input"))
        .await
        .unwrap();

    // Two open issues across all projects, newest-seen first.
    let recent = et::recent_open_issues(&pool, 8, def_org()).await.unwrap();
    assert_eq!(recent.len(), 2);
    assert!(recent.iter().all(|i| i.status == "unresolved"));

    // Histogram counts every event (3), not just issues.
    let hist = et::project_event_histogram(&pool, project.id, 168, 48)
        .await
        .unwrap();
    assert_eq!(hist.iter().map(|b| b.count).sum::<i64>(), 3);

    // Resolving an issue drops it from the recent-open feed.
    et::set_issue_status(&pool, recent[0].id, "resolved")
        .await
        .unwrap();
    assert_eq!(
        et::recent_open_issues(&pool, 8, def_org())
            .await
            .unwrap()
            .len(),
        1
    );
}

fn event_user(exc: &str, user: &str) -> ParsedEvent {
    let mut e = event(exc, "same message");
    e.raw = serde_json::json!({ "user": { "id": user } });
    e
}

#[sqlx::test(migrations = "../../migrations")]
async fn affected_users_distinct_with_counts(pool: PgPool) {
    let project = et::find_or_create_by_name(&pool, "[demo] web", def_org())
        .await
        .unwrap();
    // Same exception+message → one issue; alice twice, bob once.
    et::record_event(&pool, project.id, &event_user("Boom", "alice"))
        .await
        .unwrap();
    et::record_event(&pool, project.id, &event_user("Boom", "alice"))
        .await
        .unwrap();
    et::record_event(&pool, project.id, &event_user("Boom", "bob"))
        .await
        .unwrap();
    // An anonymous event (no user context) doesn't appear in the list.
    et::record_event(&pool, project.id, &event("Boom", "same message"))
        .await
        .unwrap();

    let iid = et::recent_open_issues(&pool, 8, def_org()).await.unwrap()[0].id;
    let users = et::issue_affected_users(&pool, iid, 50).await.unwrap();
    assert_eq!(users.len(), 2, "alice + bob, anon excluded");
    assert_eq!(users.iter().find(|u| u.ident == "alice").unwrap().events, 2);
    assert_eq!(users.iter().find(|u| u.ident == "bob").unwrap().events, 1);
}
