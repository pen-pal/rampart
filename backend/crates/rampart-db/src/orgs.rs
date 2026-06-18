//! Organization + membership queries — multi-tenancy Phase 1 foundation.
//!
//! Phase 1 is behaviour-identical: there is one org (the Default org seeded
//! by migration 0107) and nothing filters by org. These queries back the
//! org-context resolution in the auth layer and the membership invariant
//! maintained by [`crate::users::create`].

use crate::{DbError, DbPool, DbResult};
use rampart_core::ids::{OrgId, UserId};
use rampart_core::org::{Org, OrgMember};
use rampart_core::Role;
use uuid::Uuid;

/// Create an org. `slug` must match `^[a-z0-9-]{2,40}$` (DB CHECK).
pub async fn create(pool: &DbPool, slug: &str, name: &str) -> DbResult<Org> {
    let id = OrgId::new();
    let row = sqlx::query!(
        r#"
        INSERT INTO organizations (id, slug, name)
        VALUES ($1, $2, $3)
        RETURNING id, slug, name, created_at
        "#,
        id.0,
        slug,
        name,
    )
    .fetch_one(pool)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(d) if d.is_unique_violation() => {
            DbError::Conflict("org slug already taken".into())
        }
        _ => DbError::Sqlx(e),
    })?;
    Ok(Org {
        id: OrgId::from_uuid(row.id),
        slug: row.slug,
        name: row.name,
        created_at: row.created_at,
    })
}

pub async fn get(pool: &DbPool, id: OrgId) -> DbResult<Org> {
    let row = sqlx::query!(
        r#"SELECT id, slug, name, created_at FROM organizations WHERE id = $1"#,
        id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(Org {
        id: OrgId::from_uuid(row.id),
        slug: row.slug,
        name: row.name,
        created_at: row.created_at,
    })
}

/// Orgs the user belongs to, newest membership first. Backs the (future)
/// org switcher and the `/me` org list.
pub async fn list_for_user(pool: &DbPool, user_id: UserId) -> DbResult<Vec<Org>> {
    let rows = sqlx::query!(
        r#"
        SELECT o.id, o.slug, o.name, o.created_at
        FROM organizations o
        JOIN org_members m ON m.org_id = o.id
        WHERE m.user_id = $1
        ORDER BY m.created_at ASC
        "#,
        user_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| Org {
            id: OrgId::from_uuid(r.id),
            slug: r.slug,
            name: r.name,
            created_at: r.created_at,
        })
        .collect())
}

/// Add (or update the role of) a member. Idempotent on (org_id, user_id):
/// re-adding updates the role. Used by [`crate::users::create`] to seed the
/// Default-org membership and (later) by the member-invite flow.
pub async fn upsert_member(
    pool: &DbPool,
    org_id: OrgId,
    user_id: UserId,
    role: Role,
) -> DbResult<()> {
    sqlx::query!(
        r#"
        INSERT INTO org_members (org_id, user_id, role)
        VALUES ($1, $2, $3)
        ON CONFLICT (org_id, user_id) DO UPDATE SET role = EXCLUDED.role
        "#,
        org_id.0,
        user_id.0,
        role as Role,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// The user's role in an org, or `None` if they are not a member.
pub async fn member_role(
    pool: &DbPool,
    org_id: OrgId,
    user_id: UserId,
) -> DbResult<Option<Role>> {
    let row = sqlx::query!(
        r#"SELECT role AS "role: Role" FROM org_members WHERE org_id = $1 AND user_id = $2"#,
        org_id.0,
        user_id.0,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| r.role))
}

/// Every member of an org with their role, oldest first.
pub async fn list_members(pool: &DbPool, org_id: OrgId) -> DbResult<Vec<OrgMember>> {
    let rows = sqlx::query!(
        r#"
        SELECT org_id, user_id, role AS "role: Role", created_at
        FROM org_members WHERE org_id = $1 ORDER BY created_at ASC
        "#,
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| OrgMember {
            org_id: OrgId::from_uuid(r.org_id),
            user_id: UserId::from_uuid(r.user_id),
            role: r.role,
            created_at: r.created_at,
        })
        .collect())
}

/// Add a member within an existing transaction — used by `users::create` so
/// the user row and the Default-org membership commit atomically.
pub async fn upsert_member_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    user_id: Uuid,
    role: Role,
) -> DbResult<()> {
    sqlx::query!(
        r#"
        INSERT INTO org_members (org_id, user_id, role)
        VALUES ($1, $2, $3)
        ON CONFLICT (org_id, user_id) DO UPDATE SET role = EXCLUDED.role
        "#,
        org_id,
        user_id,
        role as Role,
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Rename an org. `NotFound` when the id doesn't exist. (Slug is immutable in
/// P4 — it's the stable OIDC-mapping key; renaming the display name is enough.)
pub async fn update(pool: &DbPool, id: OrgId, name: &str) -> DbResult<Org> {
    let row = sqlx::query!(
        r#"
        UPDATE organizations SET name = $2 WHERE id = $1
        RETURNING id, slug, name, created_at
        "#,
        id.0,
        name,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(Org {
        id: OrgId::from_uuid(row.id),
        slug: row.slug,
        name: row.name,
        created_at: row.created_at,
    })
}

/// Look up an org by its unique slug. Backs the OIDC claim→org mapping (4f)
/// and slug-based lookups. `NotFound` for an unknown slug.
pub async fn get_by_slug(pool: &DbPool, slug: &str) -> DbResult<Org> {
    let row = sqlx::query!(
        r#"SELECT id, slug, name, created_at FROM organizations WHERE slug = $1"#,
        slug,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(Org {
        id: OrgId::from_uuid(row.id),
        slug: row.slug,
        name: row.name,
        created_at: row.created_at,
    })
}

/// Remove a member from an org. Returns `true` if a membership row was
/// deleted, `false` if the user wasn't a member. Callers enforce last-admin
/// protection via [`count_admins`] before calling this.
pub async fn remove_member(pool: &DbPool, org_id: OrgId, user_id: UserId) -> DbResult<bool> {
    let r = sqlx::query!(
        r#"DELETE FROM org_members WHERE org_id = $1 AND user_id = $2"#,
        org_id.0,
        user_id.0,
    )
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

/// Count the Admin-role members of an org — used for last-admin protection
/// (refuse to remove or demote the final Admin, which would orphan the org).
pub async fn count_admins(pool: &DbPool, org_id: OrgId) -> DbResult<i64> {
    let n = sqlx::query_scalar!(
        r#"SELECT COUNT(*) AS "n!" FROM org_members WHERE org_id = $1 AND role = $2"#,
        org_id.0,
        Role::Admin as Role,
    )
    .fetch_one(pool)
    .await?;
    Ok(n)
}

/// Create an org and make `owner` its first Admin, atomically — so a created
/// org always has its creator as Admin even if the process dies mid-request.
/// Backs `POST /v1/orgs` (4c).
pub async fn create_with_owner(
    pool: &DbPool,
    slug: &str,
    name: &str,
    owner: UserId,
) -> DbResult<Org> {
    let id = OrgId::new();
    let mut tx = pool.begin().await?;
    let row = sqlx::query!(
        r#"
        INSERT INTO organizations (id, slug, name)
        VALUES ($1, $2, $3)
        RETURNING id, slug, name, created_at
        "#,
        id.0,
        slug,
        name,
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(d) if d.is_unique_violation() => {
            DbError::Conflict("org slug already taken".into())
        }
        _ => DbError::Sqlx(e),
    })?;
    upsert_member_tx(&mut tx, id.0, owner.0, Role::Admin).await?;
    tx.commit().await?;
    Ok(Org {
        id: OrgId::from_uuid(row.id),
        slug: row.slug,
        name: row.name,
        created_at: row.created_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::users::{create as create_user, NewUser};
    use rampart_core::org::DEFAULT_ORG_ID;
    use sqlx::PgPool;

    #[sqlx::test(migrations = "../../migrations")]
    async fn default_org_seeded(pool: PgPool) {
        let org = get(&pool, OrgId::from_uuid(DEFAULT_ORG_ID)).await.unwrap();
        assert_eq!(org.slug, "default");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn create_user_seeds_default_membership(pool: PgPool) {
        let u = create_user(
            &pool,
            NewUser {
                email: "m@example.com".into(),
                name: None,
                password_hash: "h".into(),
                role: Role::Admin,
            },
        )
        .await
        .unwrap();

        // The user is automatically a member of the Default org with their role.
        let role = member_role(&pool, OrgId::from_uuid(DEFAULT_ORG_ID), u.id)
            .await
            .unwrap();
        assert_eq!(role, Some(Role::Admin));

        let orgs = list_for_user(&pool, u.id).await.unwrap();
        assert_eq!(orgs.len(), 1);
        assert_eq!(orgs[0].id, OrgId::from_uuid(DEFAULT_ORG_ID));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn upsert_member_is_idempotent_and_updates_role(pool: PgPool) {
        let u = create_user(
            &pool,
            NewUser {
                email: "x@example.com".into(),
                name: None,
                password_hash: "h".into(),
                role: Role::Editor,
            },
        )
        .await
        .unwrap();
        let org = OrgId::from_uuid(DEFAULT_ORG_ID);
        upsert_member(&pool, org, u.id, Role::Readonly).await.unwrap();
        assert_eq!(member_role(&pool, org, u.id).await.unwrap(), Some(Role::Readonly));
        // still exactly one membership row
        assert_eq!(list_for_user(&pool, u.id).await.unwrap().len(), 1);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn rename_and_get_by_slug(pool: PgPool) {
        let o = create(&pool, "acme", "Acme Inc").await.unwrap();
        let renamed = update(&pool, o.id, "Acme Corp").await.unwrap();
        assert_eq!(renamed.name, "Acme Corp");
        assert_eq!(renamed.slug, "acme"); // slug immutable
        let by_slug = get_by_slug(&pool, "acme").await.unwrap();
        assert_eq!(by_slug.id, o.id);
        assert!(matches!(
            get_by_slug(&pool, "nope").await,
            Err(DbError::NotFound)
        ));
        assert!(matches!(
            update(&pool, OrgId::new(), "x").await,
            Err(DbError::NotFound)
        ));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn remove_member_and_count_admins(pool: PgPool) {
        let org = create(&pool, "team", "Team").await.unwrap();
        let admin = create_user(&pool, NewUser { email: "a@e.com".into(), name: None, password_hash: "h".into(), role: Role::Admin }).await.unwrap();
        let ed = create_user(&pool, NewUser { email: "b@e.com".into(), name: None, password_hash: "h".into(), role: Role::Editor }).await.unwrap();
        upsert_member(&pool, org.id, admin.id, Role::Admin).await.unwrap();
        upsert_member(&pool, org.id, ed.id, Role::Editor).await.unwrap();
        assert_eq!(count_admins(&pool, org.id).await.unwrap(), 1);
        assert!(remove_member(&pool, org.id, ed.id).await.unwrap());
        assert!(!remove_member(&pool, org.id, ed.id).await.unwrap()); // already gone
        assert_eq!(member_role(&pool, org.id, ed.id).await.unwrap(), None);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn create_with_owner_makes_creator_admin(pool: PgPool) {
        let u = create_user(&pool, NewUser { email: "o@e.com".into(), name: None, password_hash: "h".into(), role: Role::Editor }).await.unwrap();
        let org = create_with_owner(&pool, "owned", "Owned", u.id).await.unwrap();
        assert_eq!(member_role(&pool, org.id, u.id).await.unwrap(), Some(Role::Admin));
        assert_eq!(count_admins(&pool, org.id).await.unwrap(), 1);
        // duplicate slug -> Conflict
        assert!(matches!(
            create_with_owner(&pool, "owned", "Dup", u.id).await,
            Err(DbError::Conflict(_))
        ));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn set_role_mirrors_default_membership(pool: PgPool) {
        let u = create_user(&pool, NewUser { email: "r@e.com".into(), name: None, password_hash: "h".into(), role: Role::Admin }).await.unwrap();
        let def = OrgId::from_uuid(DEFAULT_ORG_ID);
        // post-creation role change must keep the Default membership in sync
        crate::users::set_role(&pool, u.id, Role::Readonly).await.unwrap();
        assert_eq!(member_role(&pool, def, u.id).await.unwrap(), Some(Role::Readonly));
        // set_admin(false) -> Editor, mirrored; set_admin(true) -> Admin, mirrored
        crate::users::set_admin(&pool, u.id, false).await.unwrap();
        assert_eq!(member_role(&pool, def, u.id).await.unwrap(), Some(Role::Editor));
        crate::users::set_admin(&pool, u.id, true).await.unwrap();
        assert_eq!(member_role(&pool, def, u.id).await.unwrap(), Some(Role::Admin));
    }
}
