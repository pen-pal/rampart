//! SQLite `users` domain — accounts, RBAC role, TOTP/2FA state, prefs, GDPR
//! anonymize. Mirrors the Postgres `crate::users` free-fn surface (the
//! `StoreUsers` seam methods) against SQLite.
//!
//! Parity notes vs Postgres:
//! - `create` / `set_admin` / `set_role` keep the Default-org membership in sync
//!   in the same transaction (the multi-tenancy invariant), like the PG path.
//! - `set_admin` / `set_role` / `disable_totp` do NOT yet revoke sessions —
//!   the SQLite `sessions` domain isn't built. Wire the revoke in once it lands
//!   (the PG versions call `sessions::delete_for_user` after commit).
//! - booleans are INTEGER 0/1, timestamps INTEGER unix-seconds, role CHECK'd
//!   TEXT (see `migrations-sqlite/0002_identity.sql`).

use super::{default_org_id_str, role_from, role_str, ts, uid};
use crate::users::{NewUser, User, UserWithHash};
use crate::{DbError, DbResult};
use rampart_core::ids::UserId;
use rampart_core::Role;
use sqlx::SqlitePool;
use uuid::Uuid;

type UserRow = (
    String,
    String,
    Option<String>,
    i64,
    String,
    i64,
    Option<i64>,
    i64,
);

/// Row shape for `get_by_email` (adds password_hash + totp_secret).
type UserHashRow = (
    String,
    String,
    Option<String>,
    String,
    i64,
    String,
    Option<String>,
    i64,
);

fn user_from(r: UserRow) -> User {
    let (id, email, name, is_admin, role, created_at, last_login_at, totp_enabled) = r;
    User {
        id: uid(&id),
        email,
        name,
        is_admin: is_admin != 0,
        role: role_from(&role),
        created_at: ts(created_at),
        last_login_at: last_login_at.map(ts),
        totp_enabled: totp_enabled != 0,
    }
}

pub async fn count(pool: &SqlitePool) -> DbResult<i64> {
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await?;
    Ok(n)
}

/// Insert a user and seed its Default-org membership atomically (the
/// every-user-is-in-an-org invariant). Email collision → `Conflict`.
pub async fn create(pool: &SqlitePool, input: NewUser) -> DbResult<User> {
    let id = Uuid::now_v7();
    let is_admin = input.role.is_admin();
    let mut tx = pool.begin().await?;
    let row: UserRow = sqlx::query_as(
        "INSERT INTO users (id, email, name, password_hash, is_admin, role)
         VALUES (?, ?, ?, ?, ?, ?)
         RETURNING id, email, name, is_admin, role, created_at, last_login_at, totp_enabled",
    )
    .bind(id.to_string())
    .bind(&input.email)
    .bind(&input.name)
    .bind(&input.password_hash)
    .bind(is_admin as i64)
    .bind(role_str(input.role))
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(d) if d.is_unique_violation() => {
            DbError::Conflict("email already registered".into())
        }
        _ => DbError::Sqlx(e),
    })?;
    sqlx::query(
        "INSERT INTO org_members (org_id, user_id, role) VALUES (?, ?, ?)
         ON CONFLICT(org_id, user_id) DO UPDATE SET role = excluded.role",
    )
    .bind(default_org_id_str())
    .bind(id.to_string())
    .bind(role_str(input.role))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(user_from(row))
}

pub async fn get_by_email(pool: &SqlitePool, email: &str) -> DbResult<UserWithHash> {
    let row: Option<UserHashRow> = sqlx::query_as(
        "SELECT id, email, name, password_hash, is_admin, role, totp_secret, totp_enabled
         FROM users WHERE email = ?",
    )
    .bind(email)
    .fetch_optional(pool)
    .await?;
    let (id, email, name, password_hash, is_admin, role, totp_secret, totp_enabled) =
        row.ok_or(DbError::NotFound)?;
    Ok(UserWithHash {
        id: uid(&id),
        email,
        name,
        password_hash,
        is_admin: is_admin != 0,
        role: role_from(&role),
        totp_secret,
        totp_enabled: totp_enabled != 0,
    })
}

pub async fn by_email(pool: &SqlitePool, email: &str) -> DbResult<Option<User>> {
    let row: Option<UserRow> = sqlx::query_as(
        "SELECT id, email, name, is_admin, role, created_at, last_login_at, totp_enabled
         FROM users WHERE email = ?",
    )
    .bind(email)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(user_from))
}

pub async fn get(pool: &SqlitePool, id: UserId) -> DbResult<User> {
    let row: Option<UserRow> = sqlx::query_as(
        "SELECT id, email, name, is_admin, role, created_at, last_login_at, totp_enabled
         FROM users WHERE id = ?",
    )
    .bind(id.0.to_string())
    .fetch_optional(pool)
    .await?;
    row.map(user_from).ok_or(DbError::NotFound)
}

pub async fn list(pool: &SqlitePool) -> DbResult<Vec<User>> {
    let rows: Vec<UserRow> = sqlx::query_as(
        "SELECT id, email, name, is_admin, role, created_at, last_login_at, totp_enabled
         FROM users ORDER BY created_at ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(user_from).collect())
}

pub async fn set_totp_secret(pool: &SqlitePool, id: UserId, secret: &str) -> DbResult<()> {
    sqlx::query("UPDATE users SET totp_secret = ?, totp_enabled = 0 WHERE id = ?")
        .bind(secret)
        .bind(id.0.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn enable_totp(pool: &SqlitePool, id: UserId) -> DbResult<()> {
    sqlx::query("UPDATE users SET totp_enabled = 1 WHERE id = ?")
        .bind(id.0.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn disable_totp(pool: &SqlitePool, id: UserId) -> DbResult<()> {
    // NB: PG also revokes sessions here; wire that in once the SQLite sessions
    // domain exists.
    sqlx::query("UPDATE users SET totp_secret = NULL, totp_enabled = 0 WHERE id = ?")
        .bind(id.0.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn mark_login(pool: &SqlitePool, id: UserId) -> DbResult<()> {
    sqlx::query("UPDATE users SET last_login_at = unixepoch() WHERE id = ?")
        .bind(id.0.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn totp_locked_until(
    pool: &SqlitePool,
    id: UserId,
) -> DbResult<Option<time::OffsetDateTime>> {
    let row: Option<(Option<i64>,)> =
        sqlx::query_as("SELECT totp_locked_until FROM users WHERE id = ?")
            .bind(id.0.to_string())
            .fetch_optional(pool)
            .await?;
    Ok(row.and_then(|(t,)| t).map(ts))
}

/// Record a failed 2FA attempt; lock for `lockout_mins` once `>= max_attempts`.
/// Returns whether the account is now locked.
pub async fn record_totp_failure(
    pool: &SqlitePool,
    id: UserId,
    max_attempts: i32,
    lockout_mins: i32,
) -> DbResult<bool> {
    let (locked,): (i64,) = sqlx::query_as(
        "UPDATE users
         SET totp_failed_attempts = totp_failed_attempts + 1,
             totp_locked_until = CASE
                 WHEN totp_failed_attempts + 1 >= ?1 THEN unixepoch() + (?2 * 60)
                 ELSE totp_locked_until END
         WHERE id = ?3
         RETURNING (totp_locked_until IS NOT NULL AND totp_locked_until > unixepoch())",
    )
    .bind(max_attempts as i64)
    .bind(lockout_mins as i64)
    .bind(id.0.to_string())
    .fetch_one(pool)
    .await?;
    Ok(locked != 0)
}

pub async fn reset_totp_failures(pool: &SqlitePool, id: UserId) -> DbResult<()> {
    sqlx::query("UPDATE users SET totp_failed_attempts = 0, totp_locked_until = NULL WHERE id = ?")
        .bind(id.0.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_admin(pool: &SqlitePool, id: UserId, is_admin: bool) -> DbResult<()> {
    let role = if is_admin { Role::Admin } else { Role::Editor };
    set_role_inner(pool, id, role, is_admin).await
}

pub async fn set_role(pool: &SqlitePool, id: UserId, role: Role) -> DbResult<()> {
    set_role_inner(pool, id, role, role.is_admin()).await
}

/// Shared body for `set_admin`/`set_role`: update the user + mirror the role
/// onto the Default-org membership in one tx (NotFound if the user is gone).
async fn set_role_inner(pool: &SqlitePool, id: UserId, role: Role, is_admin: bool) -> DbResult<()> {
    let mut tx = pool.begin().await?;
    let r = sqlx::query("UPDATE users SET role = ?, is_admin = ? WHERE id = ?")
        .bind(role_str(role))
        .bind(is_admin as i64)
        .bind(id.0.to_string())
        .execute(&mut *tx)
        .await?;
    if r.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    sqlx::query(
        "INSERT INTO org_members (org_id, user_id, role) VALUES (?, ?, ?)
         ON CONFLICT(org_id, user_id) DO UPDATE SET role = excluded.role",
    )
    .bind(default_org_id_str())
    .bind(id.0.to_string())
    .bind(role_str(role))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn delete(pool: &SqlitePool, id: UserId) -> DbResult<()> {
    let r = sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(id.0.to_string())
        .execute(pool)
        .await?;
    if r.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

/// GDPR erasure: scrub PII in place (row kept for FK/audit integrity).
pub async fn anonymize(pool: &SqlitePool, id: UserId) -> DbResult<()> {
    let r = sqlx::query(
        "UPDATE users SET
            email = 'erased-' || ?1 || '@deleted.invalid',
            name = NULL,
            password_hash = '!gdpr-erased',
            totp_secret = NULL,
            totp_enabled = 0,
            last_login_at = NULL
         WHERE id = ?1",
    )
    .bind(id.0.to_string())
    .execute(pool)
    .await?;
    if r.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

pub async fn get_prefs(pool: &SqlitePool, id: UserId) -> DbResult<serde_json::Value> {
    let row: Option<(String,)> = sqlx::query_as("SELECT prefs FROM users WHERE id = ?")
        .bind(id.0.to_string())
        .fetch_optional(pool)
        .await?;
    let (prefs,) = row.ok_or(DbError::NotFound)?;
    Ok(serde_json::from_str(&prefs).unwrap_or_else(|_| serde_json::json!({})))
}

pub async fn set_prefs(pool: &SqlitePool, id: UserId, prefs: &serde_json::Value) -> DbResult<()> {
    let r = sqlx::query("UPDATE users SET prefs = ? WHERE id = ?")
        .bind(prefs.to_string())
        .bind(id.0.to_string())
        .execute(pool)
        .await?;
    if r.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

pub async fn set_password(pool: &SqlitePool, id: UserId, hash: &str) -> DbResult<()> {
    let r = sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(hash)
        .bind(id.0.to_string())
        .execute(pool)
        .await?;
    if r.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;

    fn new_user(email: &str, role: Role) -> NewUser {
        NewUser {
            email: email.into(),
            name: Some("T".into()),
            password_hash: "h".into(),
            role,
        }
    }

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn create_seeds_default_membership_and_reads_back(pool: SqlitePool) {
        assert_eq!(count(&pool).await.unwrap(), 0);
        let u = create(&pool, new_user("a@e.com", Role::Admin)).await.unwrap();
        assert_eq!(u.role, Role::Admin);
        assert!(u.is_admin);
        assert_eq!(count(&pool).await.unwrap(), 1);

        // Default-org membership seeded with the user's role.
        let def = super::super::oid("00000000-0000-0000-0000-000000000001");
        assert_eq!(
            super::super::orgs::member_role(&pool, def, u.id).await.unwrap(),
            Some(Role::Admin)
        );

        // get / by_email / get_by_email round-trip.
        assert_eq!(get(&pool, u.id).await.unwrap().email, "a@e.com");
        assert_eq!(by_email(&pool, "a@e.com").await.unwrap().unwrap().id, u.id);
        let wh = get_by_email(&pool, "a@e.com").await.unwrap();
        assert_eq!(wh.password_hash, "h");
        // citext/NOCASE: lookup is case-insensitive.
        assert!(by_email(&pool, "A@E.com").await.unwrap().is_some());

        // Duplicate email → Conflict.
        assert!(matches!(
            create(&pool, new_user("a@e.com", Role::Editor)).await,
            Err(DbError::Conflict(_))
        ));
    }

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn set_role_mirrors_membership_and_password(pool: SqlitePool) {
        let u = create(&pool, new_user("r@e.com", Role::Editor)).await.unwrap();
        let def = super::super::oid("00000000-0000-0000-0000-000000000001");

        set_role(&pool, u.id, Role::Readonly).await.unwrap();
        assert_eq!(get(&pool, u.id).await.unwrap().role, Role::Readonly);
        assert_eq!(
            super::super::orgs::member_role(&pool, def, u.id).await.unwrap(),
            Some(Role::Readonly)
        );
        set_admin(&pool, u.id, true).await.unwrap();
        assert!(get(&pool, u.id).await.unwrap().is_admin);
        assert_eq!(
            super::super::orgs::member_role(&pool, def, u.id).await.unwrap(),
            Some(Role::Admin)
        );

        set_password(&pool, u.id, "newhash").await.unwrap();
        assert_eq!(get_by_email(&pool, "r@e.com").await.unwrap().password_hash, "newhash");
    }

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn totp_lockout_lifecycle(pool: SqlitePool) {
        let u = create(&pool, new_user("t@e.com", Role::Editor)).await.unwrap();
        set_totp_secret(&pool, u.id, "BASE32SECRET").await.unwrap();
        enable_totp(&pool, u.id).await.unwrap();
        assert!(get(&pool, u.id).await.unwrap().totp_enabled);

        // 3rd failure (max=3) locks for 5 min.
        assert!(!record_totp_failure(&pool, u.id, 3, 5).await.unwrap());
        assert!(!record_totp_failure(&pool, u.id, 3, 5).await.unwrap());
        assert!(record_totp_failure(&pool, u.id, 3, 5).await.unwrap());
        assert!(totp_locked_until(&pool, u.id).await.unwrap().is_some());

        reset_totp_failures(&pool, u.id).await.unwrap();
        assert!(totp_locked_until(&pool, u.id).await.unwrap().is_none());

        disable_totp(&pool, u.id).await.unwrap();
        assert!(!get(&pool, u.id).await.unwrap().totp_enabled);
    }

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn prefs_list_anonymize_delete(pool: SqlitePool) {
        let u = create(&pool, new_user("p@e.com", Role::Editor)).await.unwrap();
        assert_eq!(get_prefs(&pool, u.id).await.unwrap(), serde_json::json!({}));
        set_prefs(&pool, u.id, &serde_json::json!({ "theme": "dark" }))
            .await
            .unwrap();
        assert_eq!(get_prefs(&pool, u.id).await.unwrap()["theme"], "dark");

        assert_eq!(list(&pool).await.unwrap().len(), 1);

        anonymize(&pool, u.id).await.unwrap();
        let after = get(&pool, u.id).await.unwrap();
        assert!(after.email.starts_with("erased-"));
        assert!(after.name.is_none());
        // Email login no longer resolves to the original address.
        assert!(by_email(&pool, "p@e.com").await.unwrap().is_none());

        delete(&pool, u.id).await.unwrap();
        assert!(matches!(get(&pool, u.id).await, Err(DbError::NotFound)));
    }
}
