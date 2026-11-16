//! Credential-bearing settings (`smtp`, `telemetry_token`, ...) are encrypted at
//! rest on SQLite and MySQL too, as on Postgres: the stored column is the
//! sealed envelope, while reads return plaintext. Ordinary keys stay readable.
//!
//! Own test binary because the cipher is read once per process from
//! `RAMPART_SECRET_KEY`, which must be set before the first use.
#![cfg(any(feature = "sqlite", feature = "mysql"))]

use serde_json::json;

const SECRET: &str = "smtp-password-should-not-be-stored-plain";

fn set_key() {
    std::env::set_var("RAMPART_SECRET_KEY", "0".repeat(64));
    assert!(rampart_db::secrets::is_enabled());
}

fn smtp() -> serde_json::Value {
    json!({"host": "smtp.example.com", "port": 587, "encryption": "starttls",
           "username": "u", "password": SECRET, "from": "a@example.com"})
}

fn assert_sealed(raw: &str) {
    let v: serde_json::Value = serde_json::from_str(raw).unwrap();
    assert!(
        v.get("__enc1").is_some(),
        "stored value should be sealed, got: {raw}"
    );
    assert!(!raw.contains(SECRET), "plaintext secret must not be stored");
}

#[cfg(feature = "sqlite")]
#[sqlx::test(migrations = "../../migrations-sqlite")]
async fn sqlite_secret_settings_sealed_at_rest(pool: sqlx::SqlitePool) {
    use rampart_db::sqlite::settings::{get_setting, put_setting};
    set_key();
    put_setting(&pool, "smtp", &smtp()).await.unwrap();
    assert_eq!(get_setting(&pool, "smtp").await.unwrap(), Some(smtp()));
    let (raw,): (String,) = sqlx::query_as("SELECT value FROM settings WHERE key = 'smtp'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_sealed(&raw);

    // A row written unsealed by an older build still reads back.
    sqlx::query("INSERT INTO settings (key, value) VALUES ('telemetry_token', '\"legacy\"')")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        get_setting(&pool, "telemetry_token").await.unwrap(),
        Some(json!("legacy"))
    );

    // Non-secret keys are stored as-is.
    put_setting(&pool, "require_2fa", &json!("all"))
        .await
        .unwrap();
    let (raw,): (String,) = sqlx::query_as("SELECT value FROM settings WHERE key = 'require_2fa'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(raw, "\"all\"");
}

#[cfg(feature = "mysql")]
#[sqlx::test(migrations = "../../migrations-mysql")]
async fn mysql_secret_settings_sealed_at_rest(pool: sqlx::MySqlPool) {
    use rampart_db::mysql::settings::{get_setting, put_setting};
    set_key();
    put_setting(&pool, "smtp", &smtp()).await.unwrap();
    assert_eq!(get_setting(&pool, "smtp").await.unwrap(), Some(smtp()));
    let (raw,): (String,) = sqlx::query_as("SELECT value FROM settings WHERE `key` = 'smtp'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_sealed(&raw);

    sqlx::query("INSERT INTO settings (`key`, value) VALUES ('telemetry_token', '\"legacy\"')")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        get_setting(&pool, "telemetry_token").await.unwrap(),
        Some(json!("legacy"))
    );

    put_setting(&pool, "require_2fa", &json!("all"))
        .await
        .unwrap();
    let (raw,): (String,) =
        sqlx::query_as("SELECT value FROM settings WHERE `key` = 'require_2fa'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(raw, "\"all\"");
}
