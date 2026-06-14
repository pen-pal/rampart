//! System-wide key/value settings.
//!
//! The schema is intentionally generic: a single `settings` table with
//! `key TEXT PRIMARY KEY` and `value JSONB`. Type-safe wrappers live in
//! the API tier (e.g. `settings::SmtpConfig` deserialised from the
//! `smtp` row).

use crate::{DbPool, DbResult};

/// Setting keys whose value carries credentials and is therefore encrypted at
/// rest (same AES-GCM envelope as notification channel configs — see
/// [`crate::secrets`]). Sealed transparently on `put`, opened on `get`, so
/// callers (the SMTP loader, the ingest-token check) are unaffected.
const SECRET_KEYS: &[&str] = &["smtp", "telemetry_token"];

fn is_secret(key: &str) -> bool {
    SECRET_KEYS.contains(&key)
}

pub async fn get(pool: &DbPool, key: &str) -> DbResult<Option<serde_json::Value>> {
    let row = sqlx::query!(r#"SELECT value FROM settings WHERE key = $1"#, key,)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| {
        if is_secret(key) {
            crate::secrets::open(r.value)
        } else {
            r.value
        }
    }))
}

pub async fn put(pool: &DbPool, key: &str, value: &serde_json::Value) -> DbResult<()> {
    let sealed;
    let stored = if is_secret(key) {
        sealed = crate::secrets::seal(value);
        &sealed
    } else {
        value
    };
    sqlx::query!(
        r#"
        INSERT INTO settings (key, value)
        VALUES ($1, $2)
        ON CONFLICT (key) DO UPDATE
          SET value = EXCLUDED.value,
              updated_at = NOW()
        "#,
        key,
        stored,
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

#[cfg(test)]
mod tests {
    use super::is_secret;

    #[test]
    fn secret_keys_are_recognized() {
        assert!(is_secret("smtp"));
        assert!(is_secret("telemetry_token"));
        assert!(!is_secret("retention_days"));
        assert!(!is_secret("anything_else"));
    }
}
