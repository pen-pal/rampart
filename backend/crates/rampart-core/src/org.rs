//! Organization (tenant) types — multi-tenancy Phase 1 foundation.
//!
//! An `Org` is the tenant root: every tenant-scoped resource will (in later
//! phases) carry an `org_id`. Users relate to orgs many-to-many via
//! [`OrgMember`], with the RBAC role living on the membership so a user can
//! hold different roles in different orgs (the MSP / reseller case).
//!
//! Phase 1 is behaviour-identical: there is exactly one org — the
//! [`DEFAULT_ORG_ID`] "Default" org seeded by migration 0107 — and nothing
//! filters by org yet.

use crate::ids::{OrgId, UserId};
use crate::Role;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

/// The well-known Default organization. Every install starts with exactly
/// this org; all pre-multi-tenancy data, the first user, and any request
/// without a resolved org belong to it. Fixed UUID
/// `00000000-0000-0000-0000-000000000001` so it can be referenced as a
/// constant without a database lookup, matching the migration-0107 seed.
pub const DEFAULT_ORG_ID: Uuid = Uuid::from_u128(1);

/// A tenant.
#[derive(Debug, Clone, Serialize)]
pub struct Org {
    pub id: OrgId,
    pub slug: String,
    pub name: String,
    pub created_at: OffsetDateTime,
}

/// Input to create an org. `slug` must match `^[a-z0-9-]{2,40}$` (enforced by
/// the DB CHECK and validated at the API edge).
#[derive(Debug, Clone, Deserialize)]
pub struct NewOrg {
    pub slug: String,
    pub name: String,
}

/// A user's membership in an org, carrying their per-org role.
#[derive(Debug, Clone, Serialize)]
pub struct OrgMember {
    pub org_id: OrgId,
    pub user_id: UserId,
    pub role: Role,
    pub created_at: OffsetDateTime,
}

/// The Default org's typed id (the [`DEFAULT_ORG_ID`] UUID).
impl Org {
    #[inline]
    pub fn default_id() -> OrgId {
        OrgId::from_uuid(DEFAULT_ORG_ID)
    }
}
