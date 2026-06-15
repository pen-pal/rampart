//! Integration tests for the telemetry-rule state machine
//! (`rampart_db::telemetry_rules::evaluate_tick`, migration 0081).
//!
//! Exercises the per-kind windowed aggregate + the shared fire/resolve state
//! machine against real tier rows (logs + spans, which need no FK setup).

use rampart_core::metric_rule::{RuleOp, RuleTransition};
use rampart_core::telemetry_rule::{NewTelemetryRule, TelemetryRuleKind};
use rampart_db::telemetry_rules;
use sqlx::PgPool;
use uuid::Uuid;

fn rule(kind: TelemetryRuleKind, op: RuleOp, threshold: f64, window: i32) -> NewTelemetryRule {
    NewTelemetryRule {
        name: format!("{} alert", kind.as_str()),
        kind,
        target: String::new(),
        match_text: String::new(),
        min_level: 0,
        op,
        threshold,
        window_seconds: window,
        for_seconds: 0,
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

async fn insert_span(pool: &PgPool, service: &str, duration_ms: f64, status_code: i16) {
    let span_id = Uuid::new_v4().simple().to_string();
    sqlx::query!(
        r#"INSERT INTO spans
             (span_id, trace_id, parent_span_id, service_name, name, start_ns, end_ns,
              duration_ms, status_code, status_message, received_at)
           VALUES ($1, $2, NULL, $3, 'op', 0, 0, $4, $5, NULL, now())"#,
        span_id,
        Uuid::new_v4().simple().to_string(),
        service,
        duration_ms,
        status_code,
    )
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test(migrations = "../../migrations")]
async fn log_volume_fires_and_resolves(pool: PgPool) {
    // Fire when > 2 warn+ logs land in the window.
    let r = telemetry_rules::create(
        &pool,
        NewTelemetryRule {
            min_level: 13, // warn
            ..rule(TelemetryRuleKind::LogVolume, RuleOp::Gt, 2.0, 300)
        },
    )
    .await
    .unwrap();

    // Below threshold (2 warns) → no fire.
    insert_log(&pool, "api", 13, "slow").await;
    insert_log(&pool, "api", 17, "boom").await; // error
    assert!(telemetry_rules::evaluate_tick(&pool)
        .await
        .unwrap()
        .is_empty());

    // Third warn+ crosses the threshold → fire.
    insert_log(&pool, "api", 17, "boom again").await;
    let ev = telemetry_rules::evaluate_tick(&pool).await.unwrap();
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].transition, RuleTransition::Fire);
    assert_eq!(ev[0].value, Some(3.0));
    assert!(telemetry_rules::get(&pool, r.id)
        .await
        .unwrap()
        .firing_at
        .is_some());

    // Still firing → no repeat page.
    assert!(telemetry_rules::evaluate_tick(&pool)
        .await
        .unwrap()
        .is_empty());

    // Age every log out of the window → count falls to 0 → resolve.
    sqlx::query!("UPDATE logs SET ts = now() - interval '20 minutes'")
        .execute(&pool)
        .await
        .unwrap();
    let ev = telemetry_rules::evaluate_tick(&pool).await.unwrap();
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].transition, RuleTransition::Resolve);
    assert!(telemetry_rules::get(&pool, r.id)
        .await
        .unwrap()
        .firing_at
        .is_none());
}

#[sqlx::test(migrations = "../../migrations")]
async fn log_volume_min_level_and_body_filter(pool: PgPool) {
    let r = telemetry_rules::create(
        &pool,
        NewTelemetryRule {
            min_level: 17, // error+
            match_text: "timeout".to_string(),
            ..rule(TelemetryRuleKind::LogVolume, RuleOp::Gte, 1.0, 300)
        },
    )
    .await
    .unwrap();

    // A warn (below min_level) and an error without the substring → no match.
    insert_log(&pool, "api", 13, "connection timeout").await;
    insert_log(&pool, "api", 17, "disk full").await;
    assert!(telemetry_rules::evaluate_tick(&pool)
        .await
        .unwrap()
        .is_empty());

    // An error containing "timeout" → fires.
    insert_log(&pool, "api", 17, "upstream timeout after 30s").await;
    let ev = telemetry_rules::evaluate_tick(&pool).await.unwrap();
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].transition, RuleTransition::Fire);
    let _ = r;
}

#[sqlx::test(migrations = "../../migrations")]
async fn trace_latency_p95_and_no_data_resolves(pool: PgPool) {
    let r = telemetry_rules::create(
        &pool,
        rule(TelemetryRuleKind::TraceLatency, RuleOp::Gt, 500.0, 300),
    )
    .await
    .unwrap();

    // No spans → no data → not breached → nothing.
    assert!(telemetry_rules::evaluate_tick(&pool)
        .await
        .unwrap()
        .is_empty());

    // A batch whose p95 is well over 500ms → fire.
    for _ in 0..19 {
        insert_span(&pool, "api", 100.0, 1).await;
    }
    insert_span(&pool, "api", 900.0, 1).await; // tail
    insert_span(&pool, "api", 950.0, 1).await;
    let ev = telemetry_rules::evaluate_tick(&pool).await.unwrap();
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].transition, RuleTransition::Fire);

    // Age spans out → no data → resolve (not "fire forever on stale data").
    sqlx::query!("UPDATE spans SET received_at = now() - interval '20 minutes'")
        .execute(&pool)
        .await
        .unwrap();
    let ev = telemetry_rules::evaluate_tick(&pool).await.unwrap();
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].transition, RuleTransition::Resolve);
    assert_eq!(ev[0].value, None);
    assert!(telemetry_rules::get(&pool, r.id)
        .await
        .unwrap()
        .firing_at
        .is_none());
}

#[sqlx::test(migrations = "../../migrations")]
async fn trace_error_rate_percent(pool: PgPool) {
    // Fire when the error-span percentage exceeds 25%.
    telemetry_rules::create(
        &pool,
        rule(TelemetryRuleKind::TraceErrorRate, RuleOp::Gt, 25.0, 300),
    )
    .await
    .unwrap();

    // 3 ok + 1 error = 25% → not strictly greater than 25 → no fire.
    for _ in 0..3 {
        insert_span(&pool, "api", 10.0, 1).await;
    }
    insert_span(&pool, "api", 10.0, 2).await; // error
    assert!(telemetry_rules::evaluate_tick(&pool)
        .await
        .unwrap()
        .is_empty());

    // Add another error → 2/5 = 40% → fire.
    insert_span(&pool, "api", 10.0, 2).await;
    let ev = telemetry_rules::evaluate_tick(&pool).await.unwrap();
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].transition, RuleTransition::Fire);
    assert!(ev[0].value.unwrap() > 25.0);
}

#[sqlx::test(migrations = "../../migrations")]
async fn disabled_rule_is_never_evaluated(pool: PgPool) {
    telemetry_rules::create(
        &pool,
        NewTelemetryRule {
            enabled: false,
            ..rule(TelemetryRuleKind::LogVolume, RuleOp::Gt, 0.0, 300)
        },
    )
    .await
    .unwrap();
    insert_log(&pool, "api", 17, "boom").await;
    assert!(telemetry_rules::evaluate_tick(&pool)
        .await
        .unwrap()
        .is_empty());
}
