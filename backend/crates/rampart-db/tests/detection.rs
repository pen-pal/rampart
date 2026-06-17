//! Integration tests for the detection-rule engine
//! (`rampart_db::detection`, migration 0090).
//!
//! Exercises the log matcher (service / severity / regex), the threshold,
//! the watermark (no double-counting across ticks), regex validation, and the
//! findings feed / acknowledge path against real `logs` rows.

use rampart_core::detection::{DetectionSeverity, NewDetectionRule};
use rampart_db::detection;
use sqlx::PgPool;
use uuid::Uuid;

fn rule(body_regex: &str, threshold: i32) -> NewDetectionRule {
    NewDetectionRule {
        name: "test rule".to_string(),
        description: String::new(),
        severity: DetectionSeverity::High,
        service: String::new(),
        min_level: 0,
        body_regex: body_regex.to_string(),
        attr_key: String::new(),
        attr_val: String::new(),
        threshold,
        window_seconds: 300,
        cooldown_seconds: 0,
        condition: None,
        enabled: true,
        channel_ids: vec![],
        escalation_policy_id: None,
    }
}

async fn insert_log(pool: &PgPool, service: &str, severity: i16, body: &str) {
    sqlx::query!(
        r#"INSERT INTO logs (id, ts, severity, severity_text, service_name, body, attributes, received_at)
           VALUES ($1, now(), $2, NULL, $3, $4, '{}'::jsonb, now())"#,
        Uuid::now_v7(),
        severity,
        service,
        body,
    )
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test(migrations = "../../migrations")]
async fn regex_match_threshold_and_watermark(pool: PgPool) {
    let r = detection::create(&pool, rule("failed login", 2))
        .await
        .unwrap();

    // One match — below threshold of 2 → no finding, but watermark advances.
    insert_log(&pool, "auth", 9, "failed login for bob").await;
    assert!(detection::evaluate_tick(&pool).await.unwrap().is_empty());

    // The earlier log is now behind the watermark; a single fresh match is
    // still below threshold on its own → no finding.
    insert_log(&pool, "auth", 9, "failed login for alice").await;
    assert!(detection::evaluate_tick(&pool).await.unwrap().is_empty());

    // Two fresh matches in one window → finding with count 2.
    insert_log(&pool, "auth", 9, "failed login x").await;
    insert_log(&pool, "auth", 9, "failed login y").await;
    let ev = detection::evaluate_tick(&pool).await.unwrap();
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].finding.match_count, 2);
    assert_eq!(ev[0].finding.severity, DetectionSeverity::High);
    assert!(ev[0].finding.sample.as_deref().unwrap().contains("failed login"));

    // No new logs → watermark means nothing re-fires.
    assert!(detection::evaluate_tick(&pool).await.unwrap().is_empty());

    // Findings feed + open count reflect the one raised finding.
    assert_eq!(detection::open_count(&pool).await.unwrap(), 1);
    let findings = detection::list_findings(&pool, 50, true).await.unwrap();
    assert_eq!(findings.len(), 1);

    // Acknowledge clears it from the open queue but keeps the record.
    detection::ack_finding(&pool, findings[0].id).await.unwrap();
    assert_eq!(detection::open_count(&pool).await.unwrap(), 0);
    assert_eq!(detection::list_findings(&pool, 50, true).await.unwrap().len(), 0);
    assert_eq!(detection::list_findings(&pool, 50, false).await.unwrap().len(), 1);
    let _ = r;
}

#[sqlx::test(migrations = "../../migrations")]
async fn boolean_condition_tree_matches(pool: PgPool) {
    use rampart_core::DetectionCondition as C;
    // (service=auth AND body contains "failed") OR (severity >= 17 AND NOT env=dev)
    let mut nr = rule("", 1);
    nr.condition = Some(C::Or {
        conditions: vec![
            C::And {
                conditions: vec![
                    C::Service { value: "auth".into() },
                    C::BodyContains { value: "failed".into() },
                ],
            },
            C::And {
                conditions: vec![
                    C::MinLevel { value: 17 },
                    C::Not {
                        condition: Box::new(C::Attr { key: "env".into(), value: "dev".into() }),
                    },
                ],
            },
        ],
    });
    detection::create(&pool, nr).await.unwrap();

    // Branch 1: auth service + "failed" in body.
    insert_log(&pool, "auth", 9, "failed login for bob").await;
    assert_eq!(detection::evaluate_tick(&pool).await.unwrap().len(), 1);

    // Branch 2: error severity on a non-dev record (default attrs = {}).
    insert_log(&pool, "web", 17, "boom").await;
    assert_eq!(detection::evaluate_tick(&pool).await.unwrap().len(), 1);

    // Matches neither: web service, low severity, no "failed".
    insert_log(&pool, "web", 9, "all good").await;
    assert!(detection::evaluate_tick(&pool).await.unwrap().is_empty());
}

#[sqlx::test(migrations = "../../migrations")]
async fn cooldown_suppresses_repeat_findings(pool: PgPool) {
    let mut nr = rule("boom", 1);
    nr.cooldown_seconds = 600; // 10-minute suppression window
    detection::create(&pool, nr).await.unwrap();

    // First crossing raises a finding.
    insert_log(&pool, "svc", 9, "boom one").await;
    assert_eq!(detection::evaluate_tick(&pool).await.unwrap().len(), 1);

    // Further matches inside the cooldown are suppressed — the watermark still
    // advances (no re-count) but no new finding is raised.
    insert_log(&pool, "svc", 9, "boom two").await;
    assert!(detection::evaluate_tick(&pool).await.unwrap().is_empty());
    insert_log(&pool, "svc", 9, "boom three").await;
    assert!(detection::evaluate_tick(&pool).await.unwrap().is_empty());

    // Exactly one finding despite three matches across three ticks.
    assert_eq!(detection::open_count(&pool).await.unwrap(), 1);
}

#[sqlx::test(migrations = "../../migrations")]
async fn service_scope_and_severity_floor(pool: PgPool) {
    let mut nr = rule("", 1);
    nr.service = "auth".to_string();
    nr.min_level = 17; // error+
    detection::create(&pool, nr).await.unwrap();

    // Wrong service, and right service but below the severity floor → no match.
    insert_log(&pool, "web", 17, "boom").await;
    insert_log(&pool, "auth", 13, "warn only").await;
    assert!(detection::evaluate_tick(&pool).await.unwrap().is_empty());

    // Right service at error severity → fires.
    insert_log(&pool, "auth", 17, "boom").await;
    let ev = detection::evaluate_tick(&pool).await.unwrap();
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].finding.service.as_deref(), Some("auth"));
}

async fn insert_log_attr(pool: &PgPool, service: &str, severity: i16, body: &str, attrs: serde_json::Value) {
    sqlx::query!(
        r#"INSERT INTO logs (id, ts, severity, severity_text, service_name, body, attributes, received_at)
           VALUES ($1, now(), $2, NULL, $3, $4, $5, now())"#,
        Uuid::now_v7(),
        severity,
        service,
        body,
        attrs,
    )
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test(migrations = "../../migrations")]
async fn attribute_key_match(pool: PgPool) {
    let mut nr = rule("", 1);
    nr.attr_key = "event.action".to_string();
    nr.attr_val = "user.delete".to_string();
    detection::create(&pool, nr).await.unwrap();

    // Wrong action, and a log with no attributes → no match.
    insert_log_attr(&pool, "auth", 9, "a", serde_json::json!({"event.action": "user.login"})).await;
    insert_log_attr(&pool, "auth", 9, "b", serde_json::json!({})).await;
    assert!(detection::evaluate_tick(&pool).await.unwrap().is_empty());

    // Matching attribute → fires.
    insert_log_attr(&pool, "auth", 9, "c", serde_json::json!({"event.action": "user.delete"})).await;
    let ev = detection::evaluate_tick(&pool).await.unwrap();
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].finding.match_count, 1);
}

#[sqlx::test(migrations = "../../migrations")]
async fn disabled_rule_is_inert(pool: PgPool) {
    let mut nr = rule("", 1);
    nr.enabled = false;
    detection::create(&pool, nr).await.unwrap();
    insert_log(&pool, "auth", 17, "boom").await;
    assert!(detection::evaluate_tick(&pool).await.unwrap().is_empty());
}

#[sqlx::test(migrations = "../../migrations")]
async fn regex_validation(pool: PgPool) {
    assert!(detection::regex_is_valid(&pool, "").await.unwrap());
    assert!(detection::regex_is_valid(&pool, "valid.*pattern").await.unwrap());
    // Unbalanced parenthesis is an invalid POSIX regex.
    assert!(!detection::regex_is_valid(&pool, "broken(").await.unwrap());
}
