//! SQLite `settings` domain — key/value with JSON-as-TEXT values. The original
//! P1-0 spike that proved the SQLite toolchain (driver + `#[sqlx::test]`
//! fixture). Mirrors the Postgres `settings::get`/`put` key/value upsert,
//! including the secrets-at-rest envelope for credential-bearing keys.

use crate::DbResult;
use sqlx::SqlitePool;

// Same at-rest envelope as Postgres `settings::get`/`put`: credential-bearing
// keys (smtp, telemetry_token, siem_export, ...) are sealed with
// RAMPART_SECRET_KEY on write and opened on read. Rows written unsealed by an
// older build still read back (open passes non-envelope values through) and
// get sealed on their next write.
fn open_if_secret(key: &str, v: serde_json::Value) -> serde_json::Value {
    if crate::settings::is_secret(key) {
        crate::secrets::open(v)
    } else {
        v
    }
}

fn seal_if_secret(key: &str, v: &serde_json::Value) -> String {
    if crate::settings::is_secret(key) {
        crate::secrets::seal(v).to_string()
    } else {
        v.to_string()
    }
}

/// Read a settings row, decoding the JSON-as-TEXT value. `None` when absent or
/// when the stored text isn't valid JSON (defensive — a corrupt row reads as
/// missing rather than erroring the caller).
pub async fn get_setting(pool: &SqlitePool, key: &str) -> DbResult<Option<serde_json::Value>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(row
        .and_then(|(v,)| serde_json::from_str(&v).ok())
        .map(|v| open_if_secret(key, v)))
}

/// Upsert a settings row, encoding the value as JSON text.
pub async fn put_setting(pool: &SqlitePool, key: &str, value: &serde_json::Value) -> DbResult<()> {
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = datetime('now')",
    )
    .bind(key)
    .bind(seal_if_secret(key, value))
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
