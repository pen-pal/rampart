//! TOTP recovery codes — one-shot, hashed.

use crate::{DbPool, DbResult};
use rampart_core::UserId;
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Replace any existing batch for the user with a fresh set. Returns the
/// plaintext codes — only chance the caller has to show them.
pub async fn issue_batch(pool: &DbPool, user: UserId, count: usize) -> DbResult<Vec<String>> {
    let mut tx = pool.begin().await?;
    sqlx::query!(
        "DELETE FROM totp_recovery_codes WHERE user_id = $1",
        user.0,
    )
    .execute(&mut *tx)
    .await?;

    let mut plain = Vec::with_capacity(count);
    for _ in 0..count {
        let code = generate_code();
        let hash = sha256_hex(&code);
        sqlx::query!(
            r#"
            INSERT INTO totp_recovery_codes (id, user_id, code_hash)
            VALUES ($1, $2, $3)
            "#,
            Uuid::now_v7(),
            user.0,
            hash,
        )
        .execute(&mut *tx)
        .await?;
        plain.push(code);
    }
    tx.commit().await?;
    Ok(plain)
}

/// Consume a single recovery code for `user`. Returns true on success.
/// Idempotent against already-used codes (treats them as no match).
pub async fn consume(pool: &DbPool, user: UserId, code: &str) -> DbResult<bool> {
    let hash = sha256_hex(code.trim());
    let result = sqlx::query!(
        r#"
        UPDATE totp_recovery_codes
           SET used_at = NOW()
         WHERE user_id = $1 AND code_hash = $2 AND used_at IS NULL
        "#,
        user.0,
        hash,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn delete_for_user(pool: &DbPool, user: UserId) -> DbResult<()> {
    sqlx::query!(
        "DELETE FROM totp_recovery_codes WHERE user_id = $1",
        user.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn remaining(pool: &DbPool, user: UserId) -> DbResult<i64> {
    let row = sqlx::query!(
        r#"
        SELECT COUNT(*) AS "n!" FROM totp_recovery_codes
        WHERE user_id = $1 AND used_at IS NULL
        "#,
        user.0,
    )
    .fetch_one(pool)
    .await?;
    Ok(row.n)
}

fn generate_code() -> String {
    // 10 base32 chars in two groups of 5, e.g. "K3W7F-PQ2RX".
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

