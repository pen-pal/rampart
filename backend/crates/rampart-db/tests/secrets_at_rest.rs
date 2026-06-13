//! Proves notification channel `config` is encrypted at rest when
//! `RAMPART_SECRET_KEY` is set: the stored column is ciphertext, but reads
//! transparently return the original plaintext (so the notifier dispatch path,
//! which goes through `notifications::get`, is unaffected).

use rampart_core::ChannelKind;
use rampart_db::notifications::{self, NewNotification};
use sqlx::PgPool;

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
    let created = notifications::create(&pool, new_webhook(secret)).await.unwrap();

    // Read-back via the normal API path decrypts transparently.
    let got = notifications::get(&pool, created.id).await.unwrap();
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
