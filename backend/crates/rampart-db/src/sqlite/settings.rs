//! SQLite `settings` domain — key/value with JSON-as-TEXT values. The original
//! P1-0 spike that proved the SQLite toolchain (driver + `#[sqlx::test]`
//! fixture). Mirrors the Postgres `settings::get`/`put` key/value upsert minus
//! the secrets envelope (engine-agnostic; layers on later).

use crate::DbResult;
use sqlx::SqlitePool;

/// Read a settings row, decoding the JSON-as-TEXT value. `None` when absent or
/// when the stored text isn't valid JSON (defensive — a corrupt row reads as
/// missing rather than erroring the caller).
pub async fn get_setting(pool: &SqlitePool, key: &str) -> DbResult<Option<serde_json::Value>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(row.and_then(|(v,)| serde_json::from_str(&v).ok()))
}

/// Upsert a settings row, encoding the value as JSON text.
pub async fn put_setting(pool: &SqlitePool, key: &str, value: &serde_json::Value) -> DbResult<()> {
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = datetime('now')",
    )
    .bind(key)
    .bind(value.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

/// Delete a settings row (no-op if absent). Mirrors PG `settings::delete`.
pub async fn delete_setting(pool: &SqlitePool, key: &str) -> DbResult<()> {
    sqlx::query("DELETE FROM settings WHERE key = ?")
        .bind(key)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn settings_roundtrip(pool: SqlitePool) {
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

        put_setting(&pool, "k", &serde_json::json!({ "a": 2 }))
            .await
            .unwrap();
        assert_eq!(get_setting(&pool, "k").await.unwrap().unwrap()["a"], 2);
    }
}
