//! MySQL `oidc_state` domain — pre-auth OIDC login state (the per-attempt
//! `state` token mapped to its PKCE verifier + nonce). Ported from PG. ts→BIGINT;
//! `make_interval(secs=>)` → Rust cutoff; `ON CONFLICT DO NOTHING` → `INSERT IGNORE`.
//!
//! `consume` must be ONE-TIME-USE (replay-safe). PG does it with `DELETE …
//! RETURNING` under the row lock; MySQL has no DELETE-RETURNING, so we do it in
//! a tx: `SELECT … FOR UPDATE` (row lock) → capture → `DELETE` → commit. Two
//! racing callbacks for the same state: one gets the row + deletes it; the other
//! blocks on the lock, then its SELECT finds nothing → `None`. Same guarantee.

use crate::oidc_state::{Consumed, STATE_TTL_SECS};
use crate::DbResult;
use sqlx::{MySqlPool, Row};
use time::OffsetDateTime;

pub async fn stash(
    pool: &MySqlPool,
    state: &str,
    pkce_verifier: &str,
    nonce: Option<&str>,
    return_to: Option<&str>,
) -> DbResult<()> {
    let expires = OffsetDateTime::now_utc().unix_timestamp() + STATE_TTL_SECS;
    sqlx::query(
        "INSERT IGNORE INTO oidc_login_state (state, pkce_verifier, nonce, return_to, expires_at)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(state)
    .bind(pkce_verifier)
    .bind(nonce)
    .bind(return_to)
    .bind(expires)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn consume(pool: &MySqlPool, state: &str) -> DbResult<Option<Consumed>> {
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let mut tx = pool.begin().await?;
    let row = sqlx::query(
        "SELECT pkce_verifier, nonce, return_to FROM oidc_login_state
         WHERE state = ? AND expires_at > ? FOR UPDATE",
    )
    .bind(state)
    .bind(now)
    .fetch_optional(&mut *tx)
    .await?;
    let consumed = match row {
        Some(r) => {
            sqlx::query("DELETE FROM oidc_login_state WHERE state = ?")
                .bind(state)
                .execute(&mut *tx)
                .await?;
            Some(Consumed {
                pkce_verifier: r.get("pkce_verifier"),
                nonce: r.get("nonce"),
                return_to: r.get("return_to"),
            })
        }
        None => None,
    };
    tx.commit().await?;
    Ok(consumed)
}

pub async fn prune_expired(pool: &MySqlPool) -> DbResult<u64> {
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let r = sqlx::query("DELETE FROM oidc_login_state WHERE expires_at < ?")
        .bind(now)
        .execute(pool)
        .await?;
    Ok(r.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn stash_consume_one_time_use_and_prune(pool: MySqlPool) {
        stash(&pool, "st1", "verifier1", Some("nonce1"), None)
            .await
            .unwrap();
        let c = consume(&pool, "st1").await.unwrap().expect("first consume");
        assert_eq!(c.pkce_verifier, "verifier1");
        assert_eq!(c.nonce.as_deref(), Some("nonce1"));
        assert_eq!(c.return_to, None);
        // replay → None; unknown → None.
        assert!(consume(&pool, "st1").await.unwrap().is_none());
        assert!(consume(&pool, "nope").await.unwrap().is_none());

        // expired row consumes to None but is NOT deleted by consume (prune's job).
        stash(&pool, "st-exp", "v-exp", Some("n"), Some("/back"))
            .await
            .unwrap();
        sqlx::query(
            "UPDATE oidc_login_state SET expires_at = UNIX_TIMESTAMP() - 60 WHERE state = ?",
        )
        .bind("st-exp")
        .execute(&pool)
        .await
        .unwrap();
        assert!(consume(&pool, "st-exp").await.unwrap().is_none());
        stash(&pool, "st-live", "v-live", None, None).await.unwrap();
        assert_eq!(prune_expired(&pool).await.unwrap(), 1);
        assert_eq!(
            consume(&pool, "st-live")
                .await
                .unwrap()
                .unwrap()
                .pkce_verifier,
            "v-live"
        );
    }
}
