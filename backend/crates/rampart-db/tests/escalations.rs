//! Episode state machine (`rampart_db::escalations`, migration 0074).

use rampart_core::escalation::{EscalationStep, NewEscalationPolicy};
use rampart_core::ids::NotificationId;
use rampart_core::monitor::NewMonitor;
use rampart_core::{MonitorKind, Role};
use rampart_db::escalations;
use sqlx::PgPool;

async fn fixture(pool: &PgPool) -> (rampart_core::MonitorId, rampart_core::EscalationPolicy) {
    let policy = escalations::create(
        pool,
        NewEscalationPolicy {
            name: "ops ladder".into(),
            steps: vec![
                EscalationStep {
                    wait_seconds: 0,
                    channel_ids: vec![NotificationId::new()],
                },
                EscalationStep {
                    wait_seconds: 600,
                    channel_ids: vec![NotificationId::new()],
                },
            ],
        },
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
        },
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
