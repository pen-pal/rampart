//! Access-review reporting — a point-in-time snapshot of who has what role in
//! every org, with last-login + MFA status. This is the current-STATE view an
//! auditor requests for a SOC 2 CC6 (logical access) user-access review; the
//! audit log already captures the access-change EVENTS, this complements it
//! with the standing grant table.

use crate::{DbPool, DbResult};
use rampart_core::ids::{OrgId, UserId};
use rampart_core::Role;
use serde::Serialize;
use time::OffsetDateTime;

/// One (org, member) access grant enriched with the member's identity, last
/// login, and MFA status.
#[derive(Debug, Clone, Serialize)]
pub struct AccessReviewRow {
    pub org_id: OrgId,
    pub org_slug: String,
    pub org_name: String,
    pub user_id: UserId,
    pub email: String,
    pub name: Option<String>,
    pub role: Role,
    pub member_since: OffsetDateTime,
    pub last_login_at: Option<OffsetDateTime>,
    pub totp_enabled: bool,
}

/// Every membership across every org, joined to the member's identity. Ordered
/// by org name then email so the report reads like the table an auditor
/// expects. Pure read; no org filter (this is the cross-tenant admin view).
pub async fn list(pool: &DbPool) -> DbResult<Vec<AccessReviewRow>> {
    let rows = sqlx::query!(
        r#"
        SELECT m.org_id,
               o.slug AS org_slug,
               o.name AS org_name,
               m.user_id,
               u.email::text AS "email!",
               u.name,
               m.role AS "role: Role",
               m.created_at AS member_since,
               u.last_login_at,
               u.totp_enabled
        FROM org_members m
        JOIN users u ON u.id = m.user_id
        JOIN organizations o ON o.id = m.org_id
        ORDER BY o.name, u.email
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| AccessReviewRow {
            org_id: OrgId::from_uuid(r.org_id),
            org_slug: r.org_slug,
            org_name: r.org_name,
            user_id: UserId::from_uuid(r.user_id),
            email: r.email,
            name: r.name,
            role: r.role,
            member_since: r.member_since,
            last_login_at: r.last_login_at,
            totp_enabled: r.totp_enabled,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::users::{create as create_user, NewUser};
    use rampart_core::org::DEFAULT_ORG_ID;
    use sqlx::PgPool;

    #[sqlx::test(migrations = "../../migrations")]
    async fn report_lists_memberships_with_identity(pool: PgPool) {
        // Two users in the Default org with different roles.
        let admin = create_user(
            &pool,
            NewUser {
                email: "admin@e.com".into(),
                name: Some("Admin".into()),
                password_hash: "h".into(),
                role: Role::Admin,
            },
        )
        .await
        .unwrap();
        let _editor = create_user(
            &pool,
            NewUser {
                email: "editor@e.com".into(),
                name: None,
                password_hash: "h".into(),
                role: Role::Editor,
            },
        )
        .await
        .unwrap();

        let rows = list(&pool).await.unwrap();
        // Both users are seeded into the Default org by users::create.
        let def = OrgId::from_uuid(DEFAULT_ORG_ID);
        let admin_row = rows
            .iter()
            .find(|r| r.user_id == admin.id && r.org_id == def)
            .expect("admin membership present");
        assert_eq!(admin_row.role, Role::Admin);
        assert_eq!(admin_row.email, "admin@e.com");
        assert!(!admin_row.totp_enabled);
        assert!(admin_row.last_login_at.is_none());
        // The report covers every membership (>= the 2 we created).
        assert!(rows.len() >= 2);
    }
}
