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
}
