//! MySQL `recovery_codes` domain — one-shot hashed TOTP recovery codes. Ported
//! from PG. ts→BIGINT; `NOW()`→`UNIX_TIMESTAMP()`. `consume` relies on
//! rows_affected==1 (a real null→ts change, not a no-op, so MySQL counts it).

use crate::DbResult;
use rampart_core::UserId;
use sha2::{Digest, Sha256};
use sqlx::MySqlPool;
use uuid::Uuid;

/// Replace any existing batch with a fresh set; returns the plaintext codes.
pub async fn issue_batch(pool: &MySqlPool, user: UserId, count: usize) -> DbResult<Vec<String>> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM totp_recovery_codes WHERE user_id = ?")
        .bind(user.0.to_string())
        .execute(&mut *tx)
        .await?;
    let mut plain = Vec::with_capacity(count);
    for _ in 0..count {
        let code = generate_code();
        sqlx::query("INSERT INTO totp_recovery_codes (id, user_id, code_hash) VALUES (?, ?, ?)")
            .bind(Uuid::now_v7().to_string())
            .bind(user.0.to_string())
            .bind(sha256_hex(&code))
            .execute(&mut *tx)
            .await?;
        plain.push(code);
    }
    tx.commit().await?;
    Ok(plain)
}

/// Consume one code for `user`. True on success; idempotent vs used codes.
pub async fn consume(pool: &MySqlPool, user: UserId, code: &str) -> DbResult<bool> {
    let res = sqlx::query(
        "UPDATE totp_recovery_codes SET used_at = UNIX_TIMESTAMP()
         WHERE user_id = ? AND code_hash = ? AND used_at IS NULL",
    )
    .bind(user.0.to_string())
    .bind(sha256_hex(code.trim()))
    .execute(pool)
    .await?;
    Ok(res.rows_affected() == 1)
}

pub async fn delete_for_user(pool: &MySqlPool, user: UserId) -> DbResult<()> {
    sqlx::query("DELETE FROM totp_recovery_codes WHERE user_id = ?")
        .bind(user.0.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn remaining(pool: &MySqlPool, user: UserId) -> DbResult<i64> {
    let (n,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM totp_recovery_codes WHERE user_id = ? AND used_at IS NULL",
    )
    .bind(user.0.to_string())
    .fetch_one(pool)
    .await?;
    Ok(n)
}

fn generate_code() -> String {
    use rand::Rng;
    const ALPHA: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789"; // no I/O/0/1
    let mut rng = rand::thread_rng();
    let raw: String = (0..10)
        .map(|_| ALPHA[rng.gen_range(0..ALPHA.len())] as char)
        .collect();
    format!("{}-{}", &raw[..5], &raw[5..])
}

fn sha256_hex(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    hex::encode(h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn issue_consume_remaining(pool: MySqlPool) {
        let u = UserId::new();
        let codes = issue_batch(&pool, u, 5).await.unwrap();
        assert_eq!(codes.len(), 5);
        assert_eq!(remaining(&pool, u).await.unwrap(), 5);

        // consume one → true; re-consume same → false (already used); bogus → false.
        assert!(consume(&pool, u, &codes[0]).await.unwrap());
        assert!(!consume(&pool, u, &codes[0]).await.unwrap());
        assert!(!consume(&pool, u, "NOPE0-NOPE1").await.unwrap());
        assert_eq!(remaining(&pool, u).await.unwrap(), 4);

        // re-issue replaces the batch (old codes no longer valid).
        let fresh = issue_batch(&pool, u, 3).await.unwrap();
        assert_eq!(remaining(&pool, u).await.unwrap(), 3);
        assert!(!consume(&pool, u, &codes[1]).await.unwrap());
        assert!(consume(&pool, u, &fresh[0]).await.unwrap());

        delete_for_user(&pool, u).await.unwrap();
        assert_eq!(remaining(&pool, u).await.unwrap(), 0);
    }
}
