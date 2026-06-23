//! MySQL `access_review` domain — the SOC 2 CC6 user-access-review snapshot:
//! every (org, member) grant joined to the member's identity + last-login + MFA
//! status. Ported from PG. Cross-tenant admin read (no org filter). Dialect:
//! uuid→CHAR(36), role enum→TEXT (`role_from`), ts→BIGINT, bool→TINYINT.

use super::{oid, role_from, ts, uid};
use crate::access_review::AccessReviewRow;
use crate::DbResult;
use sqlx::{MySqlPool, Row};

pub async fn list(pool: &MySqlPool) -> DbResult<Vec<AccessReviewRow>> {
    let rows = sqlx::query(
        "SELECT m.org_id, o.slug AS org_slug, o.name AS org_name, m.user_id,
                u.email, u.name AS user_name, m.role, m.created_at AS member_since,
                u.last_login_at, u.totp_enabled
         FROM org_members m
         JOIN users u ON u.id = m.user_id
         JOIN organizations o ON o.id = m.org_id
         ORDER BY o.name, u.email",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| AccessReviewRow {
            org_id: oid(&r.get::<String, _>("org_id")),
            org_slug: r.get("org_slug"),
            org_name: r.get("org_name"),
            user_id: uid(&r.get::<String, _>("user_id")),
            email: r.get("email"),
            name: r.get("user_name"),
            role: role_from(&r.get::<String, _>("role")),
            member_since: ts(r.get::<i64, _>("member_since")),
            last_login_at: r.get::<Option<i64>, _>("last_login_at").map(ts),
            totp_enabled: r.get::<i64, _>("totp_enabled") != 0,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mysql::users;
    use rampart_core::Role;

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn report_lists_memberships_with_identity(pool: MySqlPool) {
        let admin = users::create(
            &pool,
            serde_json::from_value(serde_json::json!({
                "email": "admin@e.com", "name": "Admin", "password_hash": "h", "role": "admin"
            }))
            .unwrap(),
        )
        .await
        .unwrap();
        users::create(
            &pool,
            serde_json::from_value(serde_json::json!({
                "email": "editor@e.com", "password_hash": "h", "role": "editor"
            }))
            .unwrap(),
        )
        .await
        .unwrap();

        let rows = list(&pool).await.unwrap();
        let def = super::super::oid("00000000-0000-0000-0000-000000000001");
        let admin_row = rows
            .iter()
            .find(|r| r.user_id == admin.id && r.org_id == def)
            .expect("admin membership present");
        assert_eq!(admin_row.role, Role::Admin);
        assert_eq!(admin_row.email, "admin@e.com");
        assert!(!admin_row.totp_enabled);
        assert!(admin_row.last_login_at.is_none());
        assert!(rows.len() >= 2);
    }
}
