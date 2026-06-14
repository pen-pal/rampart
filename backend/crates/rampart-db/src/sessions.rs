//! Session queries.
//!
//! Sessions are server-side: the cookie value is just the session id
//! (UUID v4 — cryptographically random, unlike v7 which embeds time).
//! Every protected request looks up the row, checks `expires_at`, and
//! optionally refreshes activity.

use crate::{DbError, DbPool, DbResult};
use rampart_core::ids::UserId;
use serde::Serialize;
use std::net::IpAddr;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Session {
    pub id: Uuid,
    pub user_id: UserId,
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
        id: row.id,
        user_id: UserId::from_uuid(row.user_id),
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
        id: row.id,
        user_id: UserId::from_uuid(row.user_id),
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

/// Revoke every session for a user — called on any credential / role / 2FA
/// change so a password reset, demotion, or 2FA-disable can't leave a stale
/// (possibly compromised) session alive. Returns rows deleted.
pub async fn delete_for_user(pool: &DbPool, user_id: UserId) -> DbResult<u64> {
    let r = sqlx::query!(r#"DELETE FROM sessions WHERE user_id = $1"#, user_id.0)
        .execute(pool)
        .await?;
    Ok(r.rows_affected())
}

/// One row of the "where am I logged in" list.
#[derive(Debug, Serialize)]
pub struct SessionInfo {
    pub id: Uuid,
    pub ip_addr: Option<String>,
    pub user_agent: Option<String>,
    pub created_at: OffsetDateTime,
    pub expires_at: OffsetDateTime,
}

/// Active (unexpired) sessions for a user, newest first.
pub async fn list_for_user(pool: &DbPool, user_id: UserId) -> DbResult<Vec<SessionInfo>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, host(ip_addr) AS ip_addr, user_agent, created_at, expires_at
        FROM sessions
        WHERE user_id = $1 AND expires_at > now()
        ORDER BY created_at DESC
        "#,
        user_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| SessionInfo {
            id: r.id,
            ip_addr: r.ip_addr,
            user_agent: r.user_agent,
            created_at: r.created_at,
            expires_at: r.expires_at,
        })
        .collect())
}

/// Revoke one session, scoped to its owner (a user can't kill another's).
pub async fn delete_one_for_user(pool: &DbPool, user_id: UserId, id: Uuid) -> DbResult<bool> {
    let r = sqlx::query!(
        "DELETE FROM sessions WHERE id = $1 AND user_id = $2",
        id,
        user_id.0,
    )
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

/// Revoke all of a user's sessions except `keep` — "sign out other devices".
pub async fn delete_others(pool: &DbPool, user_id: UserId, keep: Uuid) -> DbResult<u64> {
    let r = sqlx::query!(
        "DELETE FROM sessions WHERE user_id = $1 AND id <> $2",
        user_id.0,
        keep,
    )
    .execute(pool)
    .await?;
    Ok(r.rows_affected())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::users::{create as create_user, set_password, set_role, NewUser};
    use rampart_core::Role;
    use sqlx::PgPool;

    async fn user(pool: &PgPool, email: &str) -> UserId {
        create_user(
            pool,
            NewUser {
                email: email.into(),
                name: None,
                password_hash: "hash".into(),
                role: Role::Admin,
            },
        )
        .await
        .unwrap()
        .id
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn password_change_revokes_all_sessions(pool: PgPool) {
        let uid = user(&pool, "pw@example.com").await;
        let s1 = create(&pool, uid, 3600, None, None).await.unwrap();
        let s2 = create(&pool, uid, 3600, None, None).await.unwrap();
        assert!(get(&pool, s1.id).await.is_ok());

        set_password(&pool, uid, "newhash").await.unwrap();
        assert!(
            get(&pool, s1.id).await.is_err(),
            "session 1 should be revoked"
        );
        assert!(
            get(&pool, s2.id).await.is_err(),
            "session 2 should be revoked"
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn role_change_revokes_sessions(pool: PgPool) {
        let uid = user(&pool, "role@example.com").await;
        let s = create(&pool, uid, 3600, None, None).await.unwrap();
        set_role(&pool, uid, Role::Readonly).await.unwrap();
        assert!(
            get(&pool, s.id).await.is_err(),
            "demotion should revoke sessions"
        );
    }
}
