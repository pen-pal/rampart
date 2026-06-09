//! Personal API keys.
//!
//! A key is a long random secret the caller sends as
//! `Authorization: Bearer rmp_<32 chars>`. Only the SHA-256 hash of the
//! secret lives in the DB; the raw value is shown once, at creation
//! time, in the `IssuedApiKey` payload.
//!
//! Each key carries a single authoritative `scope` (migration 0057) that
//! mirrors the RBAC role tiers at the key level: `read` (GET/HEAD only),
//! `write` (read + mutations, like an editor), `admin` (everything,
//! including admin-only routes). The legacy free-form `scopes TEXT[]` is a
//! deprecated rollback shim and is no longer read or written by the app.

use crate::ids::{ApiKeyId, UserId};
use crate::Role;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use validator::Validate;

/// Per-key authorization scope. Stored as the `scope TEXT` column on
/// `api_keys` (CHECK-constrained to these three values). Maps onto the
/// RBAC [`Role`] tiers so the existing route guards enforce it unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyScope {
    /// GET/HEAD only — least privilege, the default for new keys.
    #[default]
    Read,
    /// Read + mutations (create/update/delete) — editor-equivalent.
    Write,
    /// Everything, including the admin-only routes.
    Admin,
}

impl KeyScope {
    /// The wire / DB string form (`read` | `write` | `admin`).
    pub fn as_str(&self) -> &'static str {
        match self {
            KeyScope::Read => "read",
            KeyScope::Write => "write",
            KeyScope::Admin => "admin",
        }
    }

    /// Parse the DB / wire string. Unknown values fall back to the
    /// least-privilege `read` rather than erroring, so a stray value can
    /// never silently widen access.
    pub fn from_db(s: &str) -> Self {
        match s {
            "write" => KeyScope::Write,
            "admin" => KeyScope::Admin,
            _ => KeyScope::Read,
        }
    }

    /// Effective RBAC role this scope grants. Lets API-key auth reuse the
    /// existing role-based route guards: read→readonly, write→editor,
    /// admin→admin.
    pub fn as_role(&self) -> Role {
        match self {
            KeyScope::Read => Role::Readonly,
            KeyScope::Write => Role::Editor,
            KeyScope::Admin => Role::Admin,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: ApiKeyId,
    pub name: String,
    /// First 8 chars of the raw key — safe to display, lets the user
    /// identify which key they're looking at without exposing the
    /// secret half.
    pub key_prefix: String,
    /// Authoritative per-key authorization scope (read | write | admin).
    pub scope: KeyScope,
    pub created_by: Option<UserId>,
    pub created_at: OffsetDateTime,
    pub last_used_at: Option<OffsetDateTime>,
    pub expires_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct NewApiKey {
    #[validate(length(min = 1, max = 80))]
    pub name: String,

    /// Per-key scope. Defaults to least-privilege `read` when the client
    /// omits it.
    #[serde(default)]
    pub scope: KeyScope,

    /// Optional absolute expiry. Server validates it's in the future.
    #[serde(default)]
    pub expires_at: Option<OffsetDateTime>,
}

/// One-shot response returned by POST /v1/api-keys. The raw `token` is
/// the only chance the caller has to grab it — afterwards only the
/// prefix is queryable.
#[derive(Debug, Clone, Serialize)]
pub struct IssuedApiKey {
    pub key: ApiKey,
    /// Full plaintext token in `rmp_<32 chars>` form.
    pub token: String,
}
