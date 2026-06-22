//! Pre-auth OIDC login state (state → PKCE verifier + nonce).
//!
//! Backs the Authorization-Code-with-PKCE login flow: `/login` [`stash`]es the
//! per-attempt CSRF `state`, the PKCE `code_verifier`, and the `nonce` bound
//! into the id_token; `/callback` [`consume`]s the row atomically (one-time-use
//! under the row lock) to recover them. Replaces the previous in-process
//! HashMap so the flow survives restarts / multiple nodes.
//!
//! Rows are short-lived (`STATE_TTL_SECS`, ~10 min). [`consume`] ignores any
//! row past `expires_at`; the hourly retention sweep ([`crate::prune`]) reaps
//! the expired rows via [`prune_expired`]. This is pre-auth cross-tenant state:
//! NOT org-scoped, no RLS policy (see migration 0119).

use crate::{DbPool, DbResult};

/// How long a login-state row is valid, in seconds (10 minutes). The IdP
/// round-trip (consent screen etc.) must finish inside this window.
pub const STATE_TTL_SECS: i64 = 600;

/// Stash a fresh login state. `expires_at` is set to `now() + STATE_TTL_SECS`.
///
/// `ON CONFLICT (state) DO NOTHING`: `state` is a 48-char random token, so a
/// collision is astronomically unlikely — but if one ever happened we must NOT
/// overwrite an in-flight attempt's verifier, so we simply leave the existing
/// row. The caller's callback would then fail to match its own state and
/// restart the login, which is the safe outcome.
pub async fn stash(
    pool: &DbPool,
    state: &str,
    pkce_verifier: &str,
    nonce: Option<&str>,
    return_to: Option<&str>,
) -> DbResult<()> {
    sqlx::query!(
        r#"
        INSERT INTO oidc_login_state (state, pkce_verifier, nonce, return_to, expires_at)
        VALUES ($1, $2, $3, $4, now() + make_interval(secs => $5))
        ON CONFLICT (state) DO NOTHING
        "#,
        state,
        pkce_verifier,
        nonce,
        return_to,
        STATE_TTL_SECS as f64,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// The recovered login state, returned by [`consume`].
#[derive(Debug, Clone)]
pub struct Consumed {
    pub pkce_verifier: String,
    pub nonce: Option<String>,
    pub return_to: Option<String>,
}

/// Atomically consume the row for `state`, returning its stashed fields — or
/// `None` if the state is unknown, already consumed, or expired.
///
/// One-time-use is guaranteed by the `DELETE … RETURNING`: the row lock means
/// two concurrent callbacks racing the same `state` (a replay) can only have
/// ONE see the row; the other gets `None`. The `expires_at > now()` predicate
/// also rejects a stale row in the same statement. No CTE needed — the row
/// lock, not any read-then-delete dance, is what makes this replay-safe.
pub async fn consume(pool: &DbPool, state: &str) -> DbResult<Option<Consumed>> {
    let row = sqlx::query!(
        r#"
        DELETE FROM oidc_login_state
        WHERE state = $1 AND expires_at > now()
        RETURNING pkce_verifier, nonce, return_to
        "#,
        state,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| Consumed {
        pkce_verifier: r.pkce_verifier,
        nonce: r.nonce,
        return_to: r.return_to,
    }))
}

/// Delete all expired login-state rows. Returns the number reaped. Called from
/// the hourly retention sweep so abandoned attempts don't accumulate.
pub async fn prune_expired(pool: &DbPool) -> DbResult<u64> {
    let r = sqlx::query!("DELETE FROM oidc_login_state WHERE expires_at < now()")
        .execute(pool)
        .await?;
    Ok(r.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::PgPool;

    #[sqlx::test(migrations = "../../migrations")]
    async fn stash_consume_roundtrip_and_one_time_use(pool: PgPool) {
        // stash → consume returns the verifier + nonce.
        stash(&pool, "st1", "verifier1", Some("nonce1"), None)
            .await
            .unwrap();
        let c = consume(&pool, "st1").await.unwrap().expect("first consume");
        assert_eq!(c.pkce_verifier, "verifier1");
        assert_eq!(c.nonce.as_deref(), Some("nonce1"));
        assert_eq!(c.return_to, None);

        // Second consume of the same state → None (one-time-use under the lock).
        assert!(consume(&pool, "st1").await.unwrap().is_none());

        // Unknown state → None.
        assert!(consume(&pool, "never-stashed").await.unwrap().is_none());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn expired_row_consumes_to_none_and_prunes(pool: PgPool) {
        // Stash then force the row into the past so it is expired.
        stash(&pool, "st-exp", "verifier-exp", Some("nonce-exp"), Some("/back"))
            .await
            .unwrap();
        sqlx::query!(
            "UPDATE oidc_login_state SET expires_at = now() - make_interval(secs => 60) WHERE state = $1",
            "st-exp",
        )
        .execute(&pool)
        .await
        .unwrap();

        // An expired row consumes to None (does NOT delete it — that's prune's job).
        assert!(consume(&pool, "st-exp").await.unwrap().is_none());

        // A second, still-valid row must survive the prune.
        stash(&pool, "st-live", "verifier-live", None, None)
            .await
            .unwrap();

        // prune_expired removes the one expired row (the live one stays).
        let reaped = prune_expired(&pool).await.unwrap();
        assert_eq!(reaped, 1, "exactly the expired row is reaped");

        // The live row is still consumable.
        let c = consume(&pool, "st-live").await.unwrap().expect("live row");
        assert_eq!(c.pkce_verifier, "verifier-live");
    }
}
