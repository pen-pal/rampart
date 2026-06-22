//! User-account queries.

use crate::{DbError, DbPool, DbResult};
use rampart_core::ids::{OrgId, UserId};
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
    // The user row and its Default-org membership commit atomically: the
    // multi-tenancy invariant (every user is a member of at least one org)
    // must never be violated, even on a partial failure. Phase 1 seeds the
    // membership into the Default org with the user's role; a later phase
    // reworks provisioning to target a chosen org.
    let mut tx = pool.begin().await?;
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
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(d) if d.is_unique_violation() => {
            DbError::Conflict("email already registered".into())
        }
        _ => DbError::Sqlx(e),
    })?;

    crate::orgs::upsert_member(
        &mut *tx,
        OrgId::from_uuid(rampart_core::org::DEFAULT_ORG_ID),
        UserId::from_uuid(row.id),
        row.role,
    )
    .await?;
    tx.commit().await?;

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

/// Look up a user by email, returning the public [`User`] (no password hash
/// or TOTP secret). `None` when no such user exists. Backs the member-invite
/// flow, which resolves an *existing* user by email — it never creates one.
pub async fn by_email(pool: &DbPool, email: &str) -> DbResult<Option<User>> {
    let row = sqlx::query!(
        r#"
        SELECT id, email::text AS "email!", name, is_admin, role AS "role: Role",
               created_at, last_login_at, totp_enabled
        FROM users
        WHERE email = $1::citext
        "#,
        email,
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|row| User {
        id: UserId::from_uuid(row.id),
        email: row.email,
        name: row.name,
        is_admin: row.is_admin,
        role: row.role,
        created_at: row.created_at,
        last_login_at: row.last_login_at,
        totp_enabled: row.totp_enabled,
    }))
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
    // Disabling 2FA is a security downgrade — revoke all sessions.
    crate::sessions::delete_for_user(pool, id).await?;
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

/// When the user's 2FA step is locked out (durable brute-force cap), if at all.
/// The caller refuses the verify when this is `Some(t)` and `t > now`.
pub async fn totp_locked_until(pool: &DbPool, id: UserId) -> DbResult<Option<OffsetDateTime>> {
    let row = sqlx::query!("SELECT totp_locked_until FROM users WHERE id = $1", id.0,)
        .fetch_optional(pool)
        .await?;
    Ok(row.and_then(|r| r.totp_locked_until))
}

/// Record one failed 2FA attempt. Increments the durable counter and, once it
/// reaches `max_attempts`, sets a lockout `lockout_mins` into the future.
/// Returns `true` when the account is now locked (so the caller can refuse and
/// withhold a fresh challenge). The counter is reset by [`reset_totp_failures`]
/// on a successful verify.
pub async fn record_totp_failure(
    pool: &DbPool,
    id: UserId,
    max_attempts: i32,
    lockout_mins: i32,
) -> DbResult<bool> {
    let row = sqlx::query!(
        r#"
        UPDATE users
        SET totp_failed_attempts = totp_failed_attempts + 1,
            totp_locked_until = CASE
                WHEN totp_failed_attempts + 1 >= $2
                THEN NOW() + make_interval(mins => $3)
                ELSE totp_locked_until
            END
        WHERE id = $1
        RETURNING (totp_locked_until IS NOT NULL AND totp_locked_until > NOW()) AS "locked!"
        "#,
        id.0,
        max_attempts,
        lockout_mins,
    )
    .fetch_one(pool)
    .await?;
    Ok(row.locked)
}

/// Clear the 2FA failure counter + lockout — called after a successful verify.
pub async fn reset_totp_failures(pool: &DbPool, id: UserId) -> DbResult<()> {
    sqlx::query!(
        "UPDATE users SET totp_failed_attempts = 0, totp_locked_until = NULL WHERE id = $1",
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
    let mut tx = pool.begin().await?;
    let result = sqlx::query!(
        "UPDATE users SET is_admin = $1, role = $2 WHERE id = $3",
        is_admin,
        role as Role,
        id.0,
    )
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    // Mirror onto the Default-org membership in the same tx. Phase-4 per-org
    // RBAC sources the active-org role from `org_members`; without this mirror
    // the Default membership goes stale and single-org RBAC would enforce the
    // wrong role after any role change.
    crate::orgs::upsert_member(
        &mut *tx,
        OrgId::from_uuid(rampart_core::org::DEFAULT_ORG_ID),
        id,
        role,
    )
    .await?;
    tx.commit().await?;
    // Privilege change — revoke the target's sessions so it takes effect now.
    crate::sessions::delete_for_user(pool, id).await?;
    Ok(())
}

/// Set the authoritative RBAC role, keeping the `is_admin` shim in sync.
pub async fn set_role(pool: &DbPool, id: UserId, role: Role) -> DbResult<()> {
    let is_admin = role.is_admin();
    let mut tx = pool.begin().await?;
    let result = sqlx::query!(
        "UPDATE users SET role = $1, is_admin = $2 WHERE id = $3",
        role as Role,
        is_admin,
        id.0,
    )
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    // Mirror onto the Default-org membership (see `set_admin`).
    crate::orgs::upsert_member(
        &mut *tx,
        OrgId::from_uuid(rampart_core::org::DEFAULT_ORG_ID),
        id,
        role,
    )
    .await?;
    tx.commit().await?;
    // Privilege change — revoke the target's sessions so it takes effect now.
    crate::sessions::delete_for_user(pool, id).await?;
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

/// GDPR right-to-erasure: scrub a user's personal data IN PLACE. The row is
/// kept (not hard-deleted) so the append-only, tamper-evident audit chain and
/// every FK reference (audit `actor_user_id`, `created_by`, …) stay intact — a
/// hard DELETE is impossible anyway because `audit_log.actor_user_id` RESTRICTs.
/// Email is tombstoned (stays UNIQUE + NOT NULL), display name + 2FA secret
/// cleared, last-login dropped, and the password set to a non-verifiable
/// sentinel so the account can never authenticate. The caller separately
/// revokes the user's sessions + recovery codes. Idempotent (re-runnable).
pub async fn anonymize(pool: &DbPool, id: UserId) -> DbResult<()> {
    let result = sqlx::query!(
        r#"
        UPDATE users SET
            email         = ('erased-' || $1::text || '@deleted.invalid')::citext,
            name          = NULL,
            password_hash = '!gdpr-erased',
            totp_secret   = NULL,
            totp_enabled  = FALSE,
            last_login_at = NULL
        WHERE id = $1
        "#,
        id.0,
    )
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

/// Fetch the caller's opaque UI-preferences blob. New users (and rows that
/// predate the column's default kicking in) read back as an empty object.
pub async fn get_prefs(pool: &DbPool, id: UserId) -> DbResult<serde_json::Value> {
    let row = sqlx::query!(r#"SELECT prefs AS "prefs!" FROM users WHERE id = $1"#, id.0)
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound)?;
    Ok(row.prefs)
}

/// Overwrite the caller's preferences blob wholesale. The frontend always
/// PUTs the full document, so a replace (not a merge) is the right semantics.
pub async fn set_prefs(pool: &DbPool, id: UserId, prefs: &serde_json::Value) -> DbResult<()> {
    let result = sqlx::query!("UPDATE users SET prefs = $1 WHERE id = $2", prefs, id.0)
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
    // Revoke every session on a password change. The self-service change-
    // password route re-issues a fresh cookie so the current device stays in.
    crate::sessions::delete_for_user(pool, id).await?;
    Ok(())
}
