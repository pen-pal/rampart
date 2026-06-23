//! SQLite backend — multi-DB P1 (single-binary / homelab tier).
//!
//! Behind the `sqlite` cargo feature so the default Postgres build is untouched.
//! See `docs/design/MULTI_DB.md` (P1 plan).
//!
//! **P1-0 spike status:** proves the SQLite toolchain end-to-end on the
//! `settings` vertical — the sqlx SQLite driver and the `#[sqlx::test]`
//! per-test-DB fixture (the plan's #1 risk). NOT yet a full
//! `impl Store for SqliteStore`; that builds out domain-by-domain on this
//! foundation in later P1 slices.
//!
//! ## Why runtime-checked queries (not `query!`)
//!
//! The spike surfaced the core mechanical blocker for the SQLite backend:
//! `sqlx::query!` is compile-time-validated against ONE `DATABASE_URL`. A crate
//! holding BOTH Postgres and SQLite `query!` macros can't be validated — the PG
//! queries fail a describe against SQLite (and vice versa), and the offline
//! `.sqlx` cache can't hold both because each `cargo sqlx prepare` run prunes
//! the entries it didn't see. Gating the *entire* existing PG query layer behind
//! `cfg(feature = "postgres")` so a SQLite build excludes it is a large refactor
//! reserved for a later slice. Until then the SQLite backend uses runtime-checked
//! `sqlx::query`/`query_as` (no compile-time DB needed → compiles under
//! `SQLX_OFFLINE=1` alongside the PG cache), with correctness covered by the
//! `#[sqlx::test]` suite that runs every query against a real SQLite database.
//!
//! Dialect notes vs Postgres: `?` placeholders (not `$1`); JSON stored as TEXT
//! (no JSONB) so values round-trip through `serde_json`; `ON CONFLICT … DO
//! UPDATE SET col = excluded.col` (SQLite 3.24+, same shape as PG).

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

/// Upsert a settings row, encoding the value as JSON text. Mirrors the Postgres
/// `settings::put` key/value upsert (minus the secrets envelope, which is
/// engine-agnostic and layers on in a later slice).
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

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;

    // The SQLite analogue of the Postgres `#[sqlx::test]` fixtures: the macro
    // spins up a fresh per-test SQLite database, applies `migrations-sqlite`,
    // and injects the pool. Proves the fixture framework (P1 risk #1).
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

        // Upsert overwrites.
        put_setting(&pool, "k", &serde_json::json!({ "a": 2 }))
            .await
            .unwrap();
        assert_eq!(get_setting(&pool, "k").await.unwrap().unwrap()["a"], 2);
    }
}
