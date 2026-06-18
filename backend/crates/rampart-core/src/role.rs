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

    /// Privilege rank for ordered comparisons: `Admin (2) > Editor (1) >
    /// Readonly (0)`. A higher rank can do everything a lower one can. Used
    /// by per-org authorization to test "caller's role is at least X".
    pub fn rank(&self) -> u8 {
        match self {
            Role::Admin => 2,
            Role::Editor => 1,
            Role::Readonly => 0,
        }
    }

    /// True if this role is at least as privileged as `min`.
    pub fn at_least(&self, min: Role) -> bool {
        self.rank() >= min.rank()
    }
}
