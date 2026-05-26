//! Session queries.
//!
//! Sessions are server-side: the cookie value is just the session id
//! (UUID v4 — cryptographically random, unlike v7 which embeds time).
//! Every protected request looks up the row, checks `expires_at`, and
//! optionally refreshes activity.

use crate::{DbError, DbPool, DbResult};
use rampart_core::ids::UserId;
use std::net::IpAddr;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Session {
    pub id:         Uuid,
    pub user_id:    UserId,
    pub created_at: OffsetDateTime,
    pub expires_at: OffsetDateTime,
}

pub async fn create(
    pool: &DbPool,
    user_id: UserId,
    ttl_seconds: i64,
    ip: Option<IpAddr>,
    user_agent: Option<String>,
) -> DbResult<Session> {
    // Cryptographically random; do not use v7 (timestamp-prefixed, predictable).
    let id = Uuid::new_v4();
    let expires_at = OffsetDateTime::now_utc() + time::Duration::seconds(ttl_seconds);

    let row = sqlx::query!(
        r#"
        INSERT INTO sessions (id, user_id, expires_at, ip_addr, user_agent)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, user_id, created_at, expires_at
        "#,
        id,
        user_id.0,
        expires_at,
        ip.map(sqlx::types::ipnetwork::IpNetwork::from),
        user_agent,
    )
    .fetch_one(pool)
    .await?;

    Ok(Session {
        id:         row.id,
        user_id:    UserId::from_uuid(row.user_id),
        created_at: row.created_at,
        expires_at: row.expires_at,
    })
}

pub async fn get(pool: &DbPool, id: Uuid) -> DbResult<Session> {
    let row = sqlx::query!(
        r#"
        SELECT id, user_id, created_at, expires_at
        FROM sessions
        WHERE id = $1 AND expires_at > NOW()
        "#,
        id,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;

    Ok(Session {
        id:         row.id,
        user_id:    UserId::from_uuid(row.user_id),
        created_at: row.created_at,
        expires_at: row.expires_at,
    })
}

pub async fn delete(pool: &DbPool, id: Uuid) -> DbResult<()> {
    sqlx::query!(r#"DELETE FROM sessions WHERE id = $1"#, id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Best-effort cleanup of expired sessions. Returns rows deleted. Safe to
/// call periodically; not required for correctness (lookups already filter
/// by `expires_at`).
pub async fn cleanup_expired(pool: &DbPool) -> DbResult<u64> {
    let r = sqlx::query!(r#"DELETE FROM sessions WHERE expires_at < NOW()"#)
        .execute(pool)
        .await?;
    Ok(r.rows_affected())
}
