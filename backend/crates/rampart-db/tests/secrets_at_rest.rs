//! Proves notification channel `config` is encrypted at rest when
//! `RAMPART_SECRET_KEY` is set: the stored column is ciphertext, but reads
//! transparently return the original plaintext (so the notifier dispatch path,
//! which goes through `notifications::get`, is unaffected).

use rampart_core::ids::OrgId;
use rampart_core::monitor::NewMonitor;
use rampart_core::org::DEFAULT_ORG_ID;
use rampart_core::{ChannelKind, MonitorKind};
use rampart_db::notifications::{self, NewNotification};
use sqlx::PgPool;

const TEST_ORG: rampart_core::ids::OrgId =
    rampart_core::ids::OrgId::from_uuid(rampart_core::org::DEFAULT_ORG_ID);

fn new_webhook(secret: &str) -> NewNotification {
    NewNotification {
        kind: ChannelKind::Webhook,
        name: "enc-test".into(),
        config: serde_json::json!({ "url": "https://hooks.example.com/x", "token": secret }),
        active: true,
        template_id: None,
        cooldown_seconds: 0,
        digest_window_secs: 0,
        quiet_hours_start: None,
        quiet_hours_end: None,
        rate_limit_per_hour: 0,
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn channel_config_is_encrypted_at_rest(pool: PgPool) {
    // Must be set before the first secrets::cipher() call in this test binary.
    std::env::set_var("RAMPART_SECRET_KEY", "0".repeat(64)); // 32 bytes of 0x00 as hex

    let secret = "super-secret-bearer-token";
    let created = notifications::create(&pool, new_webhook(secret), TEST_ORG)
        .await
        .unwrap();

    // Read-back via the normal API path decrypts transparently.
    let got = notifications::get(&pool, created.id, OrgId::from_uuid(DEFAULT_ORG_ID))
        .await
        .unwrap();
    assert_eq!(got.config["token"], secret, "get() must return plaintext");

    // The raw column must NOT contain the plaintext — it's the sealed envelope.
    let raw: serde_json::Value =
        sqlx::query_scalar("SELECT config FROM notifications WHERE id = $1")
            .bind(created.id.0)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        raw.get("__enc1").is_some(),
        "stored config should be the sealed envelope, got: {raw}"
    );
    assert!(
        !raw.to_string().contains(secret),
        "plaintext secret must not appear in the stored column"
    );
}

fn http_monitor(name: &str) -> NewMonitor {
    NewMonitor {
        name: name.into(),
        kind: MonitorKind::Http,
        url: Some("https://svc.example.com".into()),
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
        escalation_policy_id: None,
        check_cert: false,
        cert_expiry_days: 14,
    }
}

/// Regression: the monitor-FLIP notifier fan-out
/// (`routing::resolve_channels_for_monitor`) must DECRYPT the channel config
/// like every other read path. Before the fix it returned the sealed envelope,
/// so live alert delivery failed ("missing field url") whenever
/// `RAMPART_SECRET_KEY` was set — encryption-at-rest silently broke alerting.
#[sqlx::test(migrations = "../../migrations")]
async fn flip_path_resolve_decrypts_channel_config(pool: PgPool) {
    std::env::set_var("RAMPART_SECRET_KEY", "0".repeat(64));

    let secret = "flip-path-bearer-token";
    let chan = notifications::create(&pool, new_webhook(secret), TEST_ORG)
        .await
        .unwrap();
    let mon = rampart_db::monitors::create(&pool, http_monitor("flip-mon"), TEST_ORG)
        .await
        .unwrap();
    notifications::attach(&pool, mon.id, chan.id).await.unwrap();

    let resolved = rampart_db::routing::resolve_channels_for_monitor(&pool, mon.id)
        .await
        .unwrap();
    assert_eq!(
        resolved.len(),
        1,
        "the attached channel resolves on the flip path"
    );
    assert_eq!(
        resolved[0].config["url"], "https://hooks.example.com/x",
        "flip path must expose the decrypted url"
    );
    assert_eq!(
        resolved[0].config["token"], secret,
        "flip path must decrypt the channel config (not return the sealed envelope)"
    );
    assert!(
        resolved[0].config.get("__enc1").is_none(),
        "must not be the sealed envelope"
    );
}
