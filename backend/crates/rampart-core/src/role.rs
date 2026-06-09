//! User RBAC role.
//!
//! Maps to the Postgres `user_role` enum (migration 0048). Three tiers:
//! `admin` (everything), `editor` (all monitoring CRUD but no admin
//! surfaces), `readonly` (GET/read only). `role` is authoritative; the
//! legacy `is_admin` boolean is a derived rollback shim.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "user_role", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    /// Default role for new users — full monitoring CRUD, no admin surfaces.
    #[default]
    Editor,
    Readonly,
}

impl Role {
    /// True if the role may perform mutating actions (create/update/delete/
    /// test). Admins and editors can; readonly cannot.
    pub fn can_write(&self) -> bool {
        matches!(self, Role::Admin | Role::Editor)
    }

    /// True only for the admin role — gates user management, settings,
    /// security, api-keys, proxies, and the audit log.
    pub fn is_admin(&self) -> bool {
        matches!(self, Role::Admin)
    }
}
