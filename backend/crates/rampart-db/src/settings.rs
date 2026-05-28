//! System-wide key/value settings.
//!
//! The schema is intentionally generic: a single `settings` table with
//! `key TEXT PRIMARY KEY` and `value JSONB`. Type-safe wrappers live in
//! the API tier (e.g. `settings::SmtpConfig` deserialised from the
//! `smtp` row).

use crate::{DbPool, DbResult};

pub async fn get(pool: &DbPool, key: &str) -> DbResult<Option<serde_json::Value>> {
    let row = sqlx::query!(r#"SELECT value FROM settings WHERE key = $1"#, key,)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| r.value))
}

pub async fn put(pool: &DbPool, key: &str, value: &serde_json::Value) -> DbResult<()> {
    sqlx::query!(
        r#"
        INSERT INTO settings (key, value)
        VALUES ($1, $2)
        ON CONFLICT (key) DO UPDATE
          SET value = EXCLUDED.value,
              updated_at = NOW()
        "#,
        key,
        value,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete(pool: &DbPool, key: &str) -> DbResult<()> {
    sqlx::query!("DELETE FROM settings WHERE key = $1", key)
        .execute(pool)
        .await?;
    Ok(())
}
