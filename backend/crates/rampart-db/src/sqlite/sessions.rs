//! SQLite `sessions` domain — server-side sessions (cookie value = session id).
//! Mirrors the Postgres `crate::sessions` free-fn surface (`StoreSessions`).
//! Session ids are random v4 UUIDs (NOT v7 — no predictable timestamp prefix).
//! Timestamps are INTEGER unix-seconds; the IP is stored as plain TEXT.

use super::{raw_uuid, ts, uid};
use crate::sessions::{Session, SessionInfo};
use crate::{DbError, DbResult};
use rampart_core::ids::UserId;
use sqlx::SqlitePool;
use std::net::IpAddr;
use time::OffsetDateTime;
use uuid::Uuid;

type SessionRow = (String, String, i64, i64, Option<String>);
/// Row shape for `list_for_user` (id, ip, ua, created, expires).
type SessionInfoRow = (String, Option<String>, Option<String>, i64, i64);
fn session_from((id, user_id, created_at, expires_at, active_org_id): SessionRow) -> Session {
    Session {
        id: raw_uuid(&id),
        user_id: uid(&user_id),
        created_at: ts(created_at),
        expires_at: ts(expires_at),
        active_org_id: active_org_id.as_deref().map(raw_uuid),
    }
}

pub async fn create(
    pool: &SqlitePool,
    user_id: UserId,
    ttl_seconds: i64,
    ip: Option<IpAddr>,
    user_agent: Option<String>,
) -> DbResult<Session> {
    let id = Uuid::new_v4();
    let expires_at = OffsetDateTime::now_utc().unix_timestamp() + ttl_seconds;
    let row: SessionRow = sqlx::query_as(
        "INSERT INTO sessions (id, user_id, expires_at, ip_addr, user_agent)
         VALUES (?, ?, ?, ?, ?)
         RETURNING id, user_id, created_at, expires_at, active_org_id",
    )
    .bind(id.to_string())
    .bind(user_id.0.to_string())
    .bind(expires_at)
    .bind(ip.map(|i| i.to_string()))
    .bind(user_agent)
    .fetch_one(pool)
    .await?;
    Ok(session_from(row))
}

pub async fn get(pool: &SqlitePool, id: Uuid) -> DbResult<Session> {
    let row: Option<SessionRow> = sqlx::query_as(
        "SELECT id, user_id, created_at, expires_at, active_org_id
         FROM sessions WHERE id = ? AND expires_at > unixepoch()",
    )
    .bind(id.to_string())
    .fetch_optional(pool)
    .await?;
    row.map(session_from).ok_or(DbError::NotFound)
}

pub async fn set_active_org(
    pool: &SqlitePool,
    session_id: Uuid,
    user_id: UserId,
    org_id: Uuid,
) -> DbResult<bool> {
    let r = sqlx::query("UPDATE sessions SET active_org_id = ? WHERE id = ? AND user_id = ?")
        .bind(org_id.to_string())
        .bind(session_id.to_string())
        .bind(user_id.0.to_string())
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn delete(pool: &SqlitePool, id: Uuid) -> DbResult<()> {
    sqlx::query("DELETE FROM sessions WHERE id = ?")
        .bind(id.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_for_user(pool: &SqlitePool, user_id: UserId) -> DbResult<u64> {
    let r = sqlx::query("DELETE FROM sessions WHERE user_id = ?")
        .bind(user_id.0.to_string())
        .execute(pool)
        .await?;
    Ok(r.rows_affected())
}

pub async fn list_for_user(pool: &SqlitePool, user_id: UserId) -> DbResult<Vec<SessionInfo>> {
    let rows: Vec<SessionInfoRow> = sqlx::query_as(
        "SELECT id, ip_addr, user_agent, created_at, expires_at
         FROM sessions WHERE user_id = ? AND expires_at > unixepoch()
         ORDER BY created_at DESC",
    )
    .bind(user_id.0.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(id, ip_addr, user_agent, created_at, expires_at)| SessionInfo {
                id: raw_uuid(&id),
                ip_addr,
                user_agent,
                created_at: ts(created_at),
                expires_at: ts(expires_at),
            },
        )
        .collect())
}

pub async fn delete_one_for_user(pool: &SqlitePool, user_id: UserId, id: Uuid) -> DbResult<bool> {
    let r = sqlx::query("DELETE FROM sessions WHERE id = ? AND user_id = ?")
        .bind(id.to_string())
        .bind(user_id.0.to_string())
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn delete_others(pool: &SqlitePool, user_id: UserId, keep: Uuid) -> DbResult<u64> {
    let r = sqlx::query("DELETE FROM sessions WHERE user_id = ? AND id <> ?")
        .bind(user_id.0.to_string())
        .bind(keep.to_string())
        .execute(pool)
        .await?;
    Ok(r.rows_affected())
}

pub async fn cleanup_expired(pool: &SqlitePool) -> DbResult<u64> {
    let r = sqlx::query("DELETE FROM sessions WHERE expires_at < unixepoch()")
        .execute(pool)
        .await?;
    Ok(r.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::users;
    use rampart_core::Role;
    use sqlx::SqlitePool;

    async fn user(pool: &SqlitePool, email: &str) -> UserId {
        users::create(
            pool,
            crate::users::NewUser {
                email: email.into(),
                name: None,
                password_hash: "h".into(),
                role: Role::Admin,
            },
        )
        .await
        .unwrap()
        .id
    }

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn create_lookup_expire_and_revoke(pool: SqlitePool) {
        let uid = user(&pool, "s@e.com").await;
        let s1 = create(
            &pool,
            uid,
            3600,
            "1.2.3.4".parse().ok(),
            Some("agent".into()),
        )
        .await
        .unwrap();
        let s2 = create(&pool, uid, 3600, None, None).await.unwrap();

        // Lookup honours expiry; ip stored as plain text.
        let got = get(&pool, s1.id).await.unwrap();
        assert_eq!(got.user_id, uid);
        let listed = list_for_user(&pool, uid).await.unwrap();
        assert_eq!(listed.len(), 2);
        assert!(listed
            .iter()
            .any(|s| s.ip_addr.as_deref() == Some("1.2.3.4")));

        // Already-expired session is invisible to get + list.
        let expired = create(&pool, uid, -10, None, None).await.unwrap();
        assert!(matches!(
            get(&pool, expired.id).await,
            Err(DbError::NotFound)
        ));
        assert_eq!(list_for_user(&pool, uid).await.unwrap().len(), 2);

        // Active-org switch (owner-scoped).
        let org = crate::sqlite::oid("00000000-0000-0000-0000-000000000001").0;
        assert!(set_active_org(&pool, s1.id, uid, org).await.unwrap());
        assert_eq!(get(&pool, s1.id).await.unwrap().active_org_id, Some(org));
        // Wrong owner can't switch.
        let other = user(&pool, "o@e.com").await;
        assert!(!set_active_org(&pool, s2.id, other, org).await.unwrap());

        // delete_others keeps s1, removing s2 + the expired row (delete_others
        // is by user_id; expiry only filters reads, not the rows themselves).
        assert_eq!(delete_others(&pool, uid, s1.id).await.unwrap(), 2);
        assert!(get(&pool, s1.id).await.is_ok());
        assert_eq!(delete_for_user(&pool, uid).await.unwrap(), 1);
        assert!(matches!(get(&pool, s1.id).await, Err(DbError::NotFound)));
    }

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn role_change_revokes_sessions(pool: SqlitePool) {
        let uid = user(&pool, "r@e.com").await;
        let s = create(&pool, uid, 3600, None, None).await.unwrap();
        // The users domain now revokes sessions on a role change (parity with PG).
        users::set_role(&pool, uid, Role::Readonly).await.unwrap();
        assert!(matches!(get(&pool, s.id).await, Err(DbError::NotFound)));
    }
}
