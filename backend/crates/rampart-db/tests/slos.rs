//! Integration tests for the SLO tier (`rampart_db::slos`, migration 0098).
//!
//! Drives the metric-ratio indicator end to end — create, compute the achieved
//! ratio / error budget over the window, and the fire/resolve budget state
//! machine — plus the CRUD + update-clears-breach behaviour. The monitor
//! indicator shares the same `evaluate_tick` machine; only the ratio source
//! differs, so it isn't re-exercised here.

use rampart_core::slo::{NewSlo, SliKind, SloTransition, UpdateSlo};
use rampart_core::promtext::PromSample;
use rampart_db::{metric_samples, slos};
use sqlx::PgPool;
use std::collections::BTreeMap;

fn sample(name: &str, value: f64) -> PromSample {
    PromSample {
        name: name.into(),
        labels: BTreeMap::new(),
        value,
    }
}

fn metric_slo(objective: f64) -> NewSlo {
    NewSlo {
        name: "api success".into(),
        description: String::new(),
        sli_kind: SliKind::Metric,
        monitor_id: None,
        good_metric: Some("req_success".into()),
        total_metric: Some("req_total".into()),
        labels: serde_json::json!({}),
        objective_pct: objective,
        window_days: 30,
        enabled: true,
        channel_ids: vec![],
        escalation_policy_id: None,
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn metric_budget_fires_and_resolves(pool: PgPool) {
    let slo = slos::create(&pool, metric_slo(99.9)).await.unwrap();

    // No samples yet → no data, no breach.
    let snap = slos::compute(&pool, &slo).await.unwrap();
    assert!(snap.achieved_pct.is_none());
    assert!(!snap.breaching);
    assert!(slos::evaluate_tick(&pool).await.unwrap().is_empty());

    // 990/1000 = 99.0% achieved against a 99.9% objective → budget blown.
    metric_samples::insert_many(
        &pool,
        &[sample("req_success", 990.0), sample("req_total", 1000.0)],
    )
    .await
    .unwrap();
    let snap = slos::compute(&pool, &slo).await.unwrap();
    assert!((snap.achieved_pct.unwrap() - 99.0).abs() < 1e-6);
    assert!(snap.breaching);
    assert_eq!(snap.remaining_pct, Some(0.0));

    // The achieved-ratio trend has a point in the current bucket near 99%.
    let tr = slos::trend(&pool, &slo, 24).await.unwrap();
    assert!(!tr.is_empty(), "trend has the recent bucket");
    assert!((tr[tr.len() - 1] - 99.0).abs() < 1e-6, "latest bucket ratio ~99%");

    // First tick fires + stamps breaching_at.
    let ev = slos::evaluate_tick(&pool).await.unwrap();
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].transition, SloTransition::Fire);
    assert!(slos::get(&pool, slo.id).await.unwrap().breaching_at.is_some());

    // Still breaching → no repeat page (de-dup).
    assert!(slos::evaluate_tick(&pool).await.unwrap().is_empty());

    // A flood of good traffic drags the windowed ratio back above objective.
    metric_samples::insert_many(
        &pool,
        &[
            sample("req_success", 1_000_000.0),
            sample("req_total", 1_000_000.0),
        ],
    )
    .await
    .unwrap();
    let snap = slos::compute(&pool, &slo).await.unwrap();
    assert!(snap.achieved_pct.unwrap() > 99.9);
    assert!(!snap.breaching);

    // Next tick resolves + clears the marker.
    let ev = slos::evaluate_tick(&pool).await.unwrap();
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].transition, SloTransition::Resolve);
    assert!(slos::get(&pool, slo.id).await.unwrap().breaching_at.is_none());

    // Quiet again → nothing.
    assert!(slos::evaluate_tick(&pool).await.unwrap().is_empty());
}

#[sqlx::test(migrations = "../../migrations")]
async fn crud_and_update_clears_breach(pool: PgPool) {
    let slo = slos::create(&pool, metric_slo(99.0)).await.unwrap();
    assert_eq!(slos::list(&pool).await.unwrap().len(), 1);

    // Force it breaching, then an edit must clear the in-flight marker.
    metric_samples::insert_many(
        &pool,
        &[sample("req_success", 50.0), sample("req_total", 100.0)],
    )
    .await
    .unwrap();
    let ev = slos::evaluate_tick(&pool).await.unwrap();
    assert_eq!(ev[0].transition, SloTransition::Fire);
    assert!(slos::get(&pool, slo.id).await.unwrap().breaching_at.is_some());

    let updated = slos::update(
        &pool,
        slo.id,
        UpdateSlo {
            name: Some("renamed".into()),
            description: None,
            good_metric: None,
            total_metric: None,
            labels: None,
            objective_pct: Some(99.5),
            window_days: None,
            enabled: None,
            channel_ids: None,
            escalation_policy_id: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(updated.name, "renamed");
    assert_eq!(updated.objective_pct, 99.5);
    assert!(updated.breaching_at.is_none());

    // Disabled SLOs are never evaluated.
    slos::update(
        &pool,
        slo.id,
        UpdateSlo {
            name: None,
            description: None,
            good_metric: None,
            total_metric: None,
            labels: None,
            objective_pct: None,
            window_days: None,
            enabled: Some(false),
            channel_ids: None,
            escalation_policy_id: None,
        },
    )
    .await
    .unwrap();
    assert!(slos::evaluate_tick(&pool).await.unwrap().is_empty());

    slos::delete(&pool, slo.id).await.unwrap();
    assert!(slos::list(&pool).await.unwrap().is_empty());
}
