//! User-account queries.

use crate::{DbError, DbPool, DbResult};
use rampart_core::ids::UserId;
use rampart_core::Role;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct User {
    pub id: UserId,
    pub email: String,
    pub name: Option<String>,
    pub is_admin: bool,
    /// Authoritative RBAC role. `is_admin` above is a derived shim kept
    /// one release for rollback safety.
    pub role: Role,
    pub created_at: OffsetDateTime,
    pub last_login_at: Option<OffsetDateTime>,
    #[serde(default)]
    pub totp_enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct UserWithHash {
    pub id: UserId,
    pub email: String,
    pub name: Option<String>,
    pub password_hash: String,
    pub is_admin: bool,
    pub role: Role,
    /// Plain TEXT secret (base32). Only loaded for login verification —
    /// never serialised back through the public User type.
    #[serde(skip_serializing)]
    pub totp_secret: Option<String>,
    pub totp_enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct NewUser {
    pub email: String,
    pub name: Option<String>,
    pub password_hash: String,
    pub role: Role,
}

pub async fn count(pool: &DbPool) -> DbResult<i64> {
    let row = sqlx::query!(r#"SELECT COUNT(*) AS "n!" FROM users"#)
        .fetch_one(pool)
        .await?;
    Ok(row.n)
}

pub async fn create(pool: &DbPool, input: NewUser) -> DbResult<User> {
    let id = Uuid::now_v7();
    // Keep the is_admin shim in sync with the authoritative role.
    let is_admin = input.role.is_admin();
    let row = sqlx::query!(
        r#"
        INSERT INTO users (id, email, name, password_hash, is_admin, role)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id, email::text AS "email!", name, is_admin, role AS "role: Role",
                  created_at, last_login_at, totp_enabled
        "#,
        id,
        input.email,
        input.name,
        input.password_hash,
        is_admin,
        input.role as Role,
    )
    .fetch_one(pool)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(d) if d.is_unique_violation() => {
            DbError::Conflict("email already registered".into())
        }
        _ => DbError::Sqlx(e),
    })?;

    Ok(User {
        id: UserId::from_uuid(row.id),
        email: row.email,
        name: row.name,
        is_admin: row.is_admin,
        role: row.role,
        created_at: row.created_at,
        last_login_at: row.last_login_at,
        totp_enabled: row.totp_enabled,
    })
}

pub async fn get_by_email(pool: &DbPool, email: &str) -> DbResult<UserWithHash> {
    let row = sqlx::query!(
        r#"
        SELECT id, email::text AS "email!", name, password_hash, is_admin,
               role AS "role: Role", totp_secret, totp_enabled
        FROM users
        WHERE email = $1::citext
        "#,
        email,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;

    Ok(UserWithHash {
        id: UserId::from_uuid(row.id),
        email: row.email,
        name: row.name,
        password_hash: row.password_hash,
        is_admin: row.is_admin,
        role: row.role,
        totp_secret: row.totp_secret,
        totp_enabled: row.totp_enabled,
    })
}

pub async fn get(pool: &DbPool, id: UserId) -> DbResult<User> {
    let row = sqlx::query!(
        r#"
        SELECT id, email::text AS "email!", name, is_admin, role AS "role: Role",
               created_at, last_login_at, totp_enabled
        FROM users
        WHERE id = $1
        "#,
        id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;

    Ok(User {
        id: UserId::from_uuid(row.id),
        email: row.email,
        name: row.name,
        is_admin: row.is_admin,
        role: row.role,
        created_at: row.created_at,
        last_login_at: row.last_login_at,
        totp_enabled: row.totp_enabled,
    })
}

/// Stash a freshly-generated TOTP secret on the user, leaving
/// `totp_enabled` FALSE — the user must confirm with a valid code
/// before we flip the flag.
pub async fn set_totp_secret(pool: &DbPool, id: UserId, secret: &str) -> DbResult<()> {
    sqlx::query!(
        "UPDATE users SET totp_secret = $1, totp_enabled = FALSE WHERE id = $2",
        secret,
        id.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn enable_totp(pool: &DbPool, id: UserId) -> DbResult<()> {
    sqlx::query!("UPDATE users SET totp_enabled = TRUE WHERE id = $1", id.0,)
        .execute(pool)
        .await?;
    Ok(())
}

/// Wipe the secret + enabled flag. Recovery codes for this user are
/// cleared by the caller (`recovery::delete_for_user`) so a future
/// enrollment starts clean.
pub async fn disable_totp(pool: &DbPool, id: UserId) -> DbResult<()> {
    sqlx::query!(
        "UPDATE users SET totp_secret = NULL, totp_enabled = FALSE WHERE id = $1",
        id.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_login(pool: &DbPool, id: UserId) -> DbResult<()> {
    sqlx::query!(
        r#"UPDATE users SET last_login_at = NOW() WHERE id = $1"#,
        id.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list(pool: &DbPool) -> DbResult<Vec<User>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, email::text AS "email!", name, is_admin, role AS "role: Role",
               created_at, last_login_at, totp_enabled
        FROM users
        ORDER BY created_at ASC
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| User {
            id: UserId::from_uuid(r.id),
            email: r.email,
            name: r.name,
            is_admin: r.is_admin,
            role: r.role,
            created_at: r.created_at,
            last_login_at: r.last_login_at,
            totp_enabled: r.totp_enabled,
        })
        .collect())
}

pub async fn set_admin(pool: &DbPool, id: UserId, is_admin: bool) -> DbResult<()> {
    // Promote/demote via the boolean shim. Keep `role` authoritative and in
    // sync: admin → Admin, !admin → Editor (the sensible non-admin default).
    let role = if is_admin { Role::Admin } else { Role::Editor };
    let result = sqlx::query!(
        "UPDATE users SET is_admin = $1, role = $2 WHERE id = $3",
        is_admin,
        role as Role,
        id.0,
    )
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

/// Set the authoritative RBAC role, keeping the `is_admin` shim in sync.
pub async fn set_role(pool: &DbPool, id: UserId, role: Role) -> DbResult<()> {
    let is_admin = role.is_admin();
    let result = sqlx::query!(
        "UPDATE users SET role = $1, is_admin = $2 WHERE id = $3",
        role as Role,
        is_admin,
        id.0,
    )
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

pub async fn delete(pool: &DbPool, id: UserId) -> DbResult<()> {
    let result = sqlx::query!("DELETE FROM users WHERE id = $1", id.0)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

pub async fn set_password(pool: &DbPool, id: UserId, hash: &str) -> DbResult<()> {
    let result = sqlx::query!(
        "UPDATE users SET password_hash = $1 WHERE id = $2",
        hash,
        id.0,
    )
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}
