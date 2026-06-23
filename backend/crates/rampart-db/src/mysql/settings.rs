//! MySQL `settings` domain — key/value with JSON-as-LONGTEXT values. The P0
//! spike that proves the MySQL toolchain (driver + `#[sqlx::test]` fixture + the
//! `ON DUPLICATE KEY UPDATE` upsert). Mirrors the PG/SQLite `settings::get`/`put`
//! minus the secrets envelope (engine-agnostic; layers on later).

use crate::DbResult;
use sqlx::MySqlPool;

/// Read a settings row, decoding the JSON text. `None` when absent or when the
/// stored text isn't valid JSON (defensive — a corrupt row reads as missing).
pub async fn get_setting(pool: &MySqlPool, key: &str) -> DbResult<Option<serde_json::Value>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE `key` = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(row.and_then(|(v,)| serde_json::from_str(&v).ok()))
}

/// Upsert a settings row, encoding the value as JSON text. `VALUES(value)` (not
/// the 8.0.20+ alias form) keeps the upsert working on MariaDB + older MySQL.
pub async fn put_setting(pool: &MySqlPool, key: &str, value: &serde_json::Value) -> DbResult<()> {
    sqlx::query(
        "INSERT INTO settings (`key`, value) VALUES (?, ?)
         ON DUPLICATE KEY UPDATE value = VALUES(value)",
    )
    .bind(key)
    .bind(value.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

/// Delete a settings row (no-op if absent). Mirrors PG `settings::delete`.
pub async fn delete_setting(pool: &MySqlPool, key: &str) -> DbResult<()> {
    sqlx::query("DELETE FROM settings WHERE `key` = ?")
        .bind(key)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Requires a MySQL server (the `backend-mysql` CI job provides one via a
    // service container + DATABASE_URL). No local MySQL → runs in CI only.
    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn settings_roundtrip(pool: MySqlPool) {
        assert!(
            get_setting(&pool, "k").await.unwrap().is_none(),
            "absent → None"
        );

        put_setting(&pool, "k", &serde_json::json!({ "a": 1, "b": "two" }))
            .await
            .unwrap();
        let got = get_setting(&pool, "k").await.unwrap().expect("present");
        assert_eq!(got["a"], 1);
        assert_eq!(got["b"], "two");

        // Upsert overwrites (ON DUPLICATE KEY).
        put_setting(&pool, "k", &serde_json::json!({ "a": 2 }))
            .await
            .unwrap();
        assert_eq!(get_setting(&pool, "k").await.unwrap().unwrap()["a"], 2);

        delete_setting(&pool, "k").await.unwrap();
        assert!(get_setting(&pool, "k").await.unwrap().is_none());
    }
}
