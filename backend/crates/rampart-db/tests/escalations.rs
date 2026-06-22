//! Episode state machine (`rampart_db::escalations`, migration 0074).

use rampart_core::escalation::{EscalationStep, NewEscalationPolicy};
use rampart_core::ids::NotificationId;
use rampart_core::monitor::NewMonitor;
use rampart_core::{MonitorKind, Role};
use rampart_db::escalations;
use sqlx::PgPool;

const TEST_ORG: rampart_core::ids::OrgId =
    rampart_core::ids::OrgId::from_uuid(rampart_core::org::DEFAULT_ORG_ID);

async fn fixture(pool: &PgPool) -> (rampart_core::MonitorId, rampart_core::EscalationPolicy) {
    let policy = escalations::create(
        pool,
        NewEscalationPolicy {
            name: "ops ladder".into(),
            steps: vec![
                EscalationStep {
                    wait_seconds: 0,
                    channel_ids: vec![NotificationId::new()],
                    schedule_ids: vec![],
                },
                EscalationStep {
                    wait_seconds: 600,
                    channel_ids: vec![NotificationId::new()],
                    schedule_ids: vec![],
                },
            ],
        },
        TEST_ORG,
    )
    .await
    .unwrap();

    let monitor = rampart_db::monitors::create(
        pool,
        NewMonitor {
            name: "web".into(),
            kind: MonitorKind::Http,
            url: Some("https://example.com".into()),
            hostname: None,
            port: None,
            config: serde_json::Value::Null,
            interval_seconds: 60,
            timeout_seconds: 10,
            max_retries: 0,
            retry_interval_sec: 60,
            resend_interval_sec: 0,
            upside_down: false,
            http_method: "GET".into(),
            http_body: None,
            http_headers: None,
            accepted_statuses: vec![200],
            follow_redirect: true,
            ignore_tls: false,
            proxy_id: None,
            group_id: None,
            slo_target_pct: None,
            slo_window_days: None,
            agent_id: None,
            escalation_policy_id: Some(policy.id),
            check_cert: false,
            cert_expiry_days: 14,
        },
        TEST_ORG,
    )
    .await
    .unwrap();
    (monitor.id, policy)
}

#[sqlx::test(migrations = "../../migrations")]
async fn open_is_idempotent_and_resolve_closes(pool: PgPool) {
    let (mid, policy) = fixture(&pool).await;

    let ep = escalations::open_episode(&pool, mid, &policy)
        .await
        .unwrap();
    assert!(ep.is_some(), "first open succeeds");
    let ep = ep.unwrap();
    assert_eq!(ep.last_step, 0);
    assert!(ep.next_escalation_at.is_some(), "step 2 scheduled");

    // Flap protection: a second Down-flip can't open a second ladder.
    assert!(escalations::open_episode(&pool, mid, &policy)
        .await
        .unwrap()
        .is_none());

    // Recovery closes it and reports how far the ladder climbed.
    let closed = escalations::resolve(&pool, mid).await.unwrap().unwrap();
    assert_eq!(closed.last_step, 0);
    assert!(escalations::resolve(&pool, mid).await.unwrap().is_none());

    // A fresh outage opens a fresh episode.
    assert!(escalations::open_episode(&pool, mid, &policy)
        .await
        .unwrap()
        .is_some());
}

#[sqlx::test(migrations = "../../migrations")]
async fn advance_fires_once_and_exhausts(pool: PgPool) {
    let (mid, policy) = fixture(&pool).await;
    let ep = escalations::open_episode(&pool, mid, &policy)
        .await
        .unwrap()
        .unwrap();

    // Not due yet (step 2 waits 600s) → the due scan is empty.
    assert!(escalations::due(&pool).await.unwrap().is_empty());

    // Backdate the deadline → due → advance claims it exactly once.
    sqlx::query!(
        "UPDATE escalation_episodes SET next_escalation_at = NOW() - INTERVAL '1 second' WHERE id = $1",
        ep.id,
    )
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(escalations::due(&pool).await.unwrap().len(), 1);

    let advanced = escalations::advance(&pool, ep.id, &policy)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(advanced.last_step, 1);
    // Two-step ladder is exhausted → no further deadline, scan empty.
    assert!(advanced.next_escalation_at.is_none());
    assert!(escalations::due(&pool).await.unwrap().is_empty());
    // A second racing advance of the same claim loses cleanly.
    assert!(escalations::advance(&pool, ep.id, &policy)
        .await
        .unwrap()
        .is_none());
}

#[sqlx::test(migrations = "../../migrations")]
async fn ack_stops_the_ladder(pool: PgPool) {
    let (mid, policy) = fixture(&pool).await;
    let ep = escalations::open_episode(&pool, mid, &policy)
        .await
        .unwrap()
        .unwrap();

    let user = rampart_db::users::create(
        &pool,
        rampart_db::users::NewUser {
            email: "oncall@example.com".into(),
            name: Some("On Call".into()),
            password_hash: "$argon2id$v=19$m=19456,t=2,p=1$fake$hash".into(),
            role: Role::Editor,
        },
    )
    .await
    .unwrap();

    let acked = escalations::ack(&pool, mid, user.id).await.unwrap();
    assert!(acked.acked_at.is_some());
    assert_eq!(acked.acked_by, Some(user.id));

    // Due scan skips acked episodes even past the deadline.
    sqlx::query!(
        "UPDATE escalation_episodes SET next_escalation_at = NOW() - INTERVAL '1 second' WHERE id = $1",
        ep.id,
    )
    .execute(&pool)
    .await
    .unwrap();
    assert!(escalations::due(&pool).await.unwrap().is_empty());

    // Double-ack is NotFound; recovery still closes an acked episode.
    assert!(escalations::ack(&pool, mid, user.id).await.is_err());
    assert!(escalations::resolve(&pool, mid).await.unwrap().is_some());
}

#[sqlx::test(migrations = "../../migrations")]
async fn rule_subject_episode_lifecycle(pool: PgPool) {
    // A non-monitor subject (telemetry rule) climbs the same ladder.
    let (_mid, policy) = fixture(&pool).await;
    let subj = "rule-abc-123";

    let ep = escalations::open_episode_for_subject(&pool, "telemetry_rule", subj, &policy)
        .await
        .unwrap()
        .expect("first open returns the episode");
    assert_eq!(ep.subject_kind, "telemetry_rule");
    assert_eq!(ep.subject_ref, subj);
    assert!(ep.monitor_id.is_none());

    // One open episode per subject.
    assert!(
        escalations::open_episode_for_subject(&pool, "telemetry_rule", subj, &policy)
            .await
            .unwrap()
            .is_none()
    );

    // Shows in the open-episodes list.
    assert!(escalations::list_open(&pool)
        .await
        .unwrap()
        .iter()
        .any(|e| e.id == ep.id));

    // Ack by episode id stops the ladder.
    let user = rampart_db::users::create(
        &pool,
        rampart_db::users::NewUser {
            email: "ruleoncall@example.com".into(),
            name: None,
            password_hash: "$argon2id$v=19$m=19456,t=2,p=1$fake$hash".into(),
            role: Role::Editor,
        },
    )
    .await
    .unwrap();
    let acked = escalations::ack_episode(&pool, ep.id, user.id)
        .await
        .unwrap();
    assert!(acked.acked_at.is_some());

    // Resolve closes it; the subject can then open a fresh episode.
    assert!(escalations::resolve_subject(&pool, "telemetry_rule", subj)
        .await
        .unwrap()
        .is_some());
    assert!(
        escalations::open_episode_for_subject(&pool, "telemetry_rule", subj, &policy)
            .await
            .unwrap()
            .is_some()
    );
}

// Build an N-rung ladder with the given per-step waits (step 0 fires at open).
async fn ladder(pool: &PgPool, waits: &[i64]) -> rampart_core::EscalationPolicy {
    escalations::create(
        pool,
        NewEscalationPolicy {
            name: "timed ladder".into(),
            steps: waits
                .iter()
                .map(|&w| EscalationStep {
                    wait_seconds: w,
                    channel_ids: vec![NotificationId::new()],
                    schedule_ids: vec![],
                })
                .collect(),
        },
        TEST_ORG,
    )
    .await
    .unwrap()
}

/// The timed climb end to end: a 4-rung ladder advances one step per elapsed
/// deadline, the next deadline always reflects the *upcoming* rung's wait, and
/// the climb exhausts at the last rung. Wall-clock is simulated by backdating
/// `next_escalation_at` (the same predicate `due`/`advance` gate on), so this
/// proves the scheduler's per-tick climb without waiting real minutes.
#[sqlx::test(migrations = "../../migrations")]
async fn multi_step_timed_climb_then_ack(pool: PgPool) {
    let waits = [0_i64, 60, 300, 900];
    let policy = ladder(&pool, &waits).await;
    let subj = "slo-timed-1";

    let ep = escalations::open_episode_for_subject(&pool, "slo", subj, &policy)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ep.last_step, 0);
    // Step 1 is scheduled but not due yet.
    assert!(ep.next_escalation_at.is_some());
    assert!(escalations::due(&pool).await.unwrap().is_empty());

    // Climb rungs 1 and 2 by fast-forwarding the clock each time.
    for expect_step in 1..=2 {
        sqlx::query!(
            "UPDATE escalation_episodes SET next_escalation_at = NOW() - INTERVAL '1 second' WHERE id = $1",
            ep.id,
        )
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(
            escalations::due(&pool).await.unwrap().len(),
            1,
            "due at step {expect_step}"
        );
        let adv = escalations::advance(&pool, ep.id, &policy)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(adv.last_step, expect_step);
        // A rung remains (step 3 of 0..=3) → a new deadline is set, not due yet.
        assert!(
            adv.next_escalation_at.is_some(),
            "rung {expect_step} schedules the next"
        );
        assert!(
            escalations::due(&pool).await.unwrap().is_empty(),
            "fresh deadline not yet due"
        );
    }

    // On-call acks at rung 2 → the climb halts even once the last deadline passes.
    let user = rampart_db::users::create(
        &pool,
        rampart_db::users::NewUser {
            email: "climb@example.com".into(),
            name: None,
            password_hash: "$argon2id$v=19$m=19456,t=2,p=1$fake$hash".into(),
            role: Role::Editor,
        },
    )
    .await
    .unwrap();
    escalations::ack_episode(&pool, ep.id, user.id)
        .await
        .unwrap();
    sqlx::query!(
        "UPDATE escalation_episodes SET next_escalation_at = NOW() - INTERVAL '1 second' WHERE id = $1",
        ep.id,
    )
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        escalations::due(&pool).await.unwrap().is_empty(),
        "acked episode never climbs again"
    );
    assert!(escalations::advance(&pool, ep.id, &policy)
        .await
        .unwrap()
        .is_none());
}

/// A climb that is never acked exhausts at the final rung: the last advance
/// leaves no further deadline.
#[sqlx::test(migrations = "../../migrations")]
async fn climb_exhausts_at_last_rung(pool: PgPool) {
    let policy = ladder(&pool, &[0_i64, 30, 60]).await;
    let ep = escalations::open_episode_for_subject(&pool, "metric_rule", "m-1", &policy)
        .await
        .unwrap()
        .unwrap();

    for step in 1..=2 {
        sqlx::query!(
            "UPDATE escalation_episodes SET next_escalation_at = NOW() - INTERVAL '1 second' WHERE id = $1",
            ep.id,
        )
        .execute(&pool)
        .await
        .unwrap();
        let adv = escalations::advance(&pool, ep.id, &policy)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(adv.last_step, step);
    }
    // Three rungs (0,1,2) climbed → exhausted, no further deadline.
    let final_ep = escalations::list_open(&pool)
        .await
        .unwrap()
        .into_iter()
        .find(|e| e.id == ep.id)
        .unwrap();
    assert_eq!(final_ep.last_step, 2);
    assert!(final_ep.next_escalation_at.is_none());
    assert!(escalations::due(&pool).await.unwrap().is_empty());
}
