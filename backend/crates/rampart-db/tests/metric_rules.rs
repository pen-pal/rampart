//! Integration tests for the metric-rule state machine
//! (`rampart_db::metric_rules::evaluate_tick`, migration 0073).

use rampart_core::ids::OrgId;
use rampart_core::metric_rule::{NewMetricRule, RuleOp, RuleTransition};
use rampart_core::org::DEFAULT_ORG_ID;
use rampart_core::promtext::PromSample;
use rampart_db::{metric_rules, metric_samples};
use sqlx::PgPool;
use std::collections::BTreeMap;

const TEST_ORG: rampart_core::ids::OrgId =
    rampart_core::ids::OrgId::from_uuid(rampart_core::org::DEFAULT_ORG_ID);

/// Rules created in tests land in the Default org (column default).
fn def_org() -> OrgId {
    OrgId::from_uuid(DEFAULT_ORG_ID)
}

fn sample(name: &str, value: f64) -> PromSample {
    PromSample {
        name: name.into(),
        labels: BTreeMap::new(),
        value,
    }
}

fn rule(metric: &str, threshold: f64, for_seconds: i32) -> NewMetricRule {
    NewMetricRule {
        name: format!("{metric} high"),
        metric: metric.into(),
        labels: serde_json::json!({}),
        op: RuleOp::Gt,
        threshold,
        for_seconds,
        enabled: true,
        channel_ids: vec![],
        escalation_policy_id: None,
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn fires_resolves_and_never_double_pages(pool: PgPool) {
    let r = metric_rules::create(&pool, rule("queue_depth", 40.0, 0), TEST_ORG)
        .await
        .unwrap();

    // Breach → immediate fire (for_seconds = 0).
    metric_samples::insert_many(&pool, &[sample("queue_depth", 42.0)], TEST_ORG)
        .await
        .unwrap();
    let events = metric_rules::evaluate_tick(&pool).await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].transition, RuleTransition::Fire);
    assert_eq!(events[0].value, Some(42.0));
    assert!(metric_rules::get(&pool, r.id, def_org())
        .await
        .unwrap()
        .firing_at
        .is_some());

    // Still breached → silent (no repeat page).
    metric_samples::insert_many(&pool, &[sample("queue_depth", 45.0)], TEST_ORG)
        .await
        .unwrap();
    assert!(metric_rules::evaluate_tick(&pool).await.unwrap().is_empty());

    // Back under → resolve once, state cleared.
    metric_samples::insert_many(&pool, &[sample("queue_depth", 12.0)], TEST_ORG)
        .await
        .unwrap();
    let events = metric_rules::evaluate_tick(&pool).await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].transition, RuleTransition::Resolve);
    let fresh = metric_rules::get(&pool, r.id, def_org()).await.unwrap();
    assert!(fresh.firing_at.is_none());
    assert!(fresh.breach_since.is_none());

    // Quiet again → nothing.
    assert!(metric_rules::evaluate_tick(&pool).await.unwrap().is_empty());
}

#[sqlx::test(migrations = "../../migrations")]
async fn sustain_window_gates_firing(pool: PgPool) {
    let r = metric_rules::create(&pool, rule("cpu_pct", 90.0, 300), TEST_ORG)
        .await
        .unwrap();
    metric_samples::insert_many(&pool, &[sample("cpu_pct", 99.0)], TEST_ORG)
        .await
        .unwrap();

    // First breached tick: pending, no alert.
    assert!(metric_rules::evaluate_tick(&pool).await.unwrap().is_empty());
    let pending = metric_rules::get(&pool, r.id, def_org()).await.unwrap();
    assert!(pending.breach_since.is_some());
    assert!(pending.firing_at.is_none());

    // Still inside the window: silent.
    assert!(metric_rules::evaluate_tick(&pool).await.unwrap().is_empty());

    // Backdate the breach past the window → next tick fires.
    sqlx::query!(
        "UPDATE metric_rules SET breach_since = NOW() - INTERVAL '301 seconds' WHERE id = $1",
        r.id.0,
    )
    .execute(&pool)
    .await
    .unwrap();
    let events = metric_rules::evaluate_tick(&pool).await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].transition, RuleTransition::Fire);

    // Recovery before-fire path: new rule, breach, then recover inside
    // the window → silent clear, never an alert.
    let r2 = metric_rules::create(&pool, rule("mem_pct", 90.0, 300), TEST_ORG)
        .await
        .unwrap();
    metric_samples::insert_many(&pool, &[sample("mem_pct", 95.0)], TEST_ORG)
        .await
        .unwrap();
    assert!(metric_rules::evaluate_tick(&pool).await.unwrap().is_empty());
    metric_samples::insert_many(&pool, &[sample("mem_pct", 50.0)], TEST_ORG)
        .await
        .unwrap();
    assert!(metric_rules::evaluate_tick(&pool).await.unwrap().is_empty());
    assert!(metric_rules::get(&pool, r2.id, def_org())
        .await
        .unwrap()
        .breach_since
        .is_none());
}

#[sqlx::test(migrations = "../../migrations")]
async fn stale_series_resolves_instead_of_firing_forever(pool: PgPool) {
    let r = metric_rules::create(&pool, rule("batch_lag", 100.0, 0), TEST_ORG)
        .await
        .unwrap();
    metric_samples::insert_many(&pool, &[sample("batch_lag", 500.0)], TEST_ORG)
        .await
        .unwrap();
    let events = metric_rules::evaluate_tick(&pool).await.unwrap();
    assert_eq!(events.len(), 1);

    // Age the sample past the freshness window — the series went dark.
    sqlx::query!("UPDATE metric_samples SET ts = NOW() - INTERVAL '20 minutes'")
        .execute(&pool)
        .await
        .unwrap();
    let events = metric_rules::evaluate_tick(&pool).await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].transition, RuleTransition::Resolve);
    assert_eq!(events[0].value, None);
    assert!(metric_rules::get(&pool, r.id, def_org())
        .await
        .unwrap()
        .firing_at
        .is_none());

    // Disabled rules are never evaluated.
    metric_rules::update(
        &pool,
        r.id,
        rampart_core::metric_rule::UpdateMetricRule {
            name: None,
            metric: None,
            labels: None,
            op: None,
            threshold: None,
            for_seconds: None,
            enabled: Some(false),
            channel_ids: None,
            escalation_policy_id: None,
        },
        def_org(),
    )
    .await
    .unwrap();
    metric_samples::insert_many(&pool, &[sample("batch_lag", 999.0)], TEST_ORG)
        .await
        .unwrap();
    assert!(metric_rules::evaluate_tick(&pool).await.unwrap().is_empty());
}
