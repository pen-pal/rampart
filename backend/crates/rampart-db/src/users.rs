//! User-account queries.

use crate::{DbError, DbPool, DbResult};
use rampart_core::ids::UserId;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct User {
    pub id: UserId,
    pub email: String,
    pub name: Option<String>,
    pub is_admin: bool,
    pub created_at: OffsetDateTime,
    pub last_login_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UserWithHash {
    pub id: UserId,
    pub email: String,
    pub name: Option<String>,
    pub password_hash: String,
    pub is_admin: bool,
}

#[derive(Debug, Deserialize)]
pub struct NewUser {
    pub email: String,
    pub name: Option<String>,
    pub password_hash: String,
    pub is_admin: bool,
}

pub async fn count(pool: &DbPool) -> DbResult<i64> {
    let row = sqlx::query!(r#"SELECT COUNT(*) AS "n!" FROM users"#)
        .fetch_one(pool)
        .await?;
    Ok(row.n)
}

pub async fn create(pool: &DbPool, input: NewUser) -> DbResult<User> {
    let id = Uuid::now_v7();
    let row = sqlx::query!(
        r#"
        INSERT INTO users (id, email, name, password_hash, is_admin)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, email::text AS "email!", name, is_admin, created_at, last_login_at
        "#,
        id,
        input.email,
        input.name,
        input.password_hash,
        input.is_admin,
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
        created_at: row.created_at,
        last_login_at: row.last_login_at,
    })
}

pub async fn get_by_email(pool: &DbPool, email: &str) -> DbResult<UserWithHash> {
    let row = sqlx::query!(
        r#"
        SELECT id, email::text AS "email!", name, password_hash, is_admin
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
    })
}

pub async fn get(pool: &DbPool, id: UserId) -> DbResult<User> {
    let row = sqlx::query!(
        r#"
        SELECT id, email::text AS "email!", name, is_admin, created_at, last_login_at
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
        created_at: row.created_at,
        last_login_at: row.last_login_at,
    })
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
