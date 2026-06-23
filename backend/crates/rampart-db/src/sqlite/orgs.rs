//! SQLite `orgs` domain — organizations + membership. Mirrors the Postgres
//! `crate::orgs` free-fn surface (the `StoreOrgs` seam methods) against SQLite.
//! The identity/tenancy core: every tenant-scoped table keys off `org_id`.

use super::{oid, role_from, role_str, ts, uid};
use crate::{DbError, DbResult};
use rampart_core::ids::{OrgId, UserId};
use rampart_core::org::{Org, OrgMember};
use rampart_core::Role;
use sqlx::SqlitePool;

/// Map a write error to `Conflict` on a unique-slug collision, else pass through.
fn slug_conflict(e: sqlx::Error) -> DbError {
    match &e {
        sqlx::Error::Database(d) if d.is_unique_violation() => {
            DbError::Conflict("org slug already taken".into())
        }
        _ => DbError::Sqlx(e),
    }
}

type OrgRow = (String, String, String, i64);
fn org_from((id, slug, name, created_at): OrgRow) -> Org {
    Org {
        id: oid(&id),
        slug,
        name,
        created_at: ts(created_at),
    }
}

pub async fn create(pool: &SqlitePool, slug: &str, name: &str) -> DbResult<Org> {
    let id = OrgId::new();
    let row: OrgRow = sqlx::query_as(
        "INSERT INTO organizations (id, slug, name) VALUES (?, ?, ?)
         RETURNING id, slug, name, created_at",
    )
    .bind(id.0.to_string())
    .bind(slug)
    .bind(name)
    .fetch_one(pool)
    .await
    .map_err(slug_conflict)?;
    Ok(org_from(row))
}

pub async fn get(pool: &SqlitePool, id: OrgId) -> DbResult<Org> {
    let row: Option<OrgRow> =
        sqlx::query_as("SELECT id, slug, name, created_at FROM organizations WHERE id = ?")
            .bind(id.0.to_string())
            .fetch_optional(pool)
            .await?;
    row.map(org_from).ok_or(DbError::NotFound)
}

pub async fn get_by_slug(pool: &SqlitePool, slug: &str) -> DbResult<Org> {
    let row: Option<OrgRow> =
        sqlx::query_as("SELECT id, slug, name, created_at FROM organizations WHERE slug = ?")
            .bind(slug)
            .fetch_optional(pool)
            .await?;
    row.map(org_from).ok_or(DbError::NotFound)
}

pub async fn update(pool: &SqlitePool, id: OrgId, name: &str) -> DbResult<Org> {
    let row: Option<OrgRow> = sqlx::query_as(
        "UPDATE organizations SET name = ? WHERE id = ?
         RETURNING id, slug, name, created_at",
    )
    .bind(name)
    .bind(id.0.to_string())
    .fetch_optional(pool)
    .await?;
    row.map(org_from).ok_or(DbError::NotFound)
}

pub async fn list_for_user(pool: &SqlitePool, user_id: UserId) -> DbResult<Vec<Org>> {
    let rows: Vec<OrgRow> = sqlx::query_as(
        "SELECT o.id, o.slug, o.name, o.created_at
         FROM organizations o JOIN org_members m ON m.org_id = o.id
         WHERE m.user_id = ? ORDER BY m.created_at ASC",
    )
    .bind(user_id.0.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(org_from).collect())
}

pub async fn upsert_member(
    pool: &SqlitePool,
    org_id: OrgId,
    user_id: UserId,
    role: Role,
) -> DbResult<()> {
    sqlx::query(
        "INSERT INTO org_members (org_id, user_id, role) VALUES (?, ?, ?)
         ON CONFLICT(org_id, user_id) DO UPDATE SET role = excluded.role",
    )
    .bind(org_id.0.to_string())
    .bind(user_id.0.to_string())
    .bind(role_str(role))
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn member_role(
    pool: &SqlitePool,
    org_id: OrgId,
    user_id: UserId,
) -> DbResult<Option<Role>> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT role FROM org_members WHERE org_id = ? AND user_id = ?")
            .bind(org_id.0.to_string())
            .bind(user_id.0.to_string())
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(r,)| role_from(&r)))
}

pub async fn list_members(pool: &SqlitePool, org_id: OrgId) -> DbResult<Vec<OrgMember>> {
    let rows: Vec<(String, String, String, i64)> = sqlx::query_as(
        "SELECT org_id, user_id, role, created_at FROM org_members
         WHERE org_id = ? ORDER BY created_at ASC",
    )
    .bind(org_id.0.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(o, u, r, c)| OrgMember {
            org_id: oid(&o),
            user_id: uid(&u),
            role: role_from(&r),
            created_at: ts(c),
        })
        .collect())
}

pub async fn list_members_detailed(
    pool: &SqlitePool,
    org_id: OrgId,
) -> DbResult<Vec<crate::orgs::OrgMemberDetail>> {
    let rows: Vec<(String, String, Option<String>, String, i64)> = sqlx::query_as(
        "SELECT m.user_id, u.email, u.name, m.role, m.created_at
         FROM org_members m JOIN users u ON u.id = m.user_id
         WHERE m.org_id = ? ORDER BY m.created_at ASC",
    )
    .bind(org_id.0.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(user_id, email, name, role, created_at)| crate::orgs::OrgMemberDetail {
                user_id: uid(&user_id),
                email,
                name,
                role: role_from(&role),
                created_at: ts(created_at),
            },
        )
        .collect())
}

pub async fn remove_member(pool: &SqlitePool, org_id: OrgId, user_id: UserId) -> DbResult<bool> {
    let r = sqlx::query("DELETE FROM org_members WHERE org_id = ? AND user_id = ?")
        .bind(org_id.0.to_string())
        .bind(user_id.0.to_string())
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn count_admins(pool: &SqlitePool, org_id: OrgId) -> DbResult<i64> {
    let (n,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM org_members WHERE org_id = ? AND role = 'admin'")
            .bind(org_id.0.to_string())
            .fetch_one(pool)
            .await?;
    Ok(n)
}

/// Create an org and make `owner` its first Admin, atomically.
pub async fn create_with_owner(
    pool: &SqlitePool,
    slug: &str,
    name: &str,
    owner: UserId,
) -> DbResult<Org> {
    let id = OrgId::new();
    let mut tx = pool.begin().await?;
    let row: OrgRow = sqlx::query_as(
        "INSERT INTO organizations (id, slug, name) VALUES (?, ?, ?)
         RETURNING id, slug, name, created_at",
    )
    .bind(id.0.to_string())
    .bind(slug)
    .bind(name)
    .fetch_one(&mut *tx)
    .await
    .map_err(slug_conflict)?;
    sqlx::query(
        "INSERT INTO org_members (org_id, user_id, role) VALUES (?, ?, 'admin')
         ON CONFLICT(org_id, user_id) DO UPDATE SET role = excluded.role",
    )
    .bind(id.0.to_string())
    .bind(owner.0.to_string())
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(org_from(row))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;
    use uuid::Uuid;

    // org_members.user_id FKs users(id) + list_members_detailed JOINs users, so
    // seed a minimal user row directly (the SQLite `users` domain lands next slice).
    async fn seed_user(pool: &SqlitePool, email: &str) -> UserId {
        let id = UserId::from_uuid(Uuid::now_v7());
        sqlx::query("INSERT INTO users (id, email, password_hash) VALUES (?, ?, '!')")
            .bind(id.0.to_string())
            .bind(email)
            .execute(pool)
            .await
            .unwrap();
        id
    }

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn create_get_by_slug_and_rename(pool: SqlitePool) {
        let o = create(&pool, "acme", "Acme Inc").await.unwrap();
        let renamed = update(&pool, o.id, "Acme Corp").await.unwrap();
        assert_eq!(renamed.name, "Acme Corp");
        assert_eq!(renamed.slug, "acme"); // slug immutable
        assert_eq!(get_by_slug(&pool, "acme").await.unwrap().id, o.id);
        assert!(matches!(
            get_by_slug(&pool, "nope").await,
            Err(DbError::NotFound)
        ));
        // Duplicate slug → Conflict.
        assert!(matches!(
            create(&pool, "acme", "Dup").await,
            Err(DbError::Conflict(_))
        ));
    }

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn membership_upsert_role_and_lists(pool: SqlitePool) {
        let org = create(&pool, "team", "Team").await.unwrap();
        let u = seed_user(&pool, "m@e.com").await;

        upsert_member(&pool, org.id, u, Role::Editor).await.unwrap();
        assert_eq!(member_role(&pool, org.id, u).await.unwrap(), Some(Role::Editor));
        // Idempotent + role update.
        upsert_member(&pool, org.id, u, Role::Readonly).await.unwrap();
        assert_eq!(
            member_role(&pool, org.id, u).await.unwrap(),
            Some(Role::Readonly)
        );
        assert_eq!(list_members(&pool, org.id).await.unwrap().len(), 1);
        assert_eq!(list_for_user(&pool, u).await.unwrap().len(), 1);

        let detailed = list_members_detailed(&pool, org.id).await.unwrap();
        assert_eq!(detailed.len(), 1);
        assert_eq!(detailed[0].email, "m@e.com");
        assert_eq!(detailed[0].role, Role::Readonly);
    }

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn remove_member_and_count_admins(pool: SqlitePool) {
        let org = create(&pool, "ops", "Ops").await.unwrap();
        let a = seed_user(&pool, "a@e.com").await;
        let e = seed_user(&pool, "e@e.com").await;
        upsert_member(&pool, org.id, a, Role::Admin).await.unwrap();
        upsert_member(&pool, org.id, e, Role::Editor).await.unwrap();
        assert_eq!(count_admins(&pool, org.id).await.unwrap(), 1);
        assert!(remove_member(&pool, org.id, e).await.unwrap());
        assert!(!remove_member(&pool, org.id, e).await.unwrap()); // already gone
        assert_eq!(member_role(&pool, org.id, e).await.unwrap(), None);
    }

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn create_with_owner_makes_admin(pool: SqlitePool) {
        let u = seed_user(&pool, "o@e.com").await;
        let org = create_with_owner(&pool, "owned", "Owned", u).await.unwrap();
        assert_eq!(member_role(&pool, org.id, u).await.unwrap(), Some(Role::Admin));
        assert_eq!(count_admins(&pool, org.id).await.unwrap(), 1);
        assert!(matches!(
            create_with_owner(&pool, "owned", "Dup", u).await,
            Err(DbError::Conflict(_))
        ));
    }

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn default_org_seeded(pool: SqlitePool) {
        let def = OrgId::from_uuid(
            Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(),
        );
        assert_eq!(get(&pool, def).await.unwrap().slug, "default");
    }
}
