//! Personal API keys.
//!
//! A key is a long random secret the caller sends as
//! `Authorization: Bearer rmp_<32 chars>`. Only the SHA-256 hash of the
//! secret lives in the DB; the raw value is shown once, at creation
//! time, in the `IssuedApiKey` payload.
//!
//! Scope strings are advisory only in v1 (we accept any) — the schema's
//! `scopes TEXT[]` is in place so future routes can gate on them.

use crate::ids::{ApiKeyId, UserId};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use validator::Validate;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub id:           ApiKeyId,
    pub name:         String,
    /// First 8 chars of the raw key — safe to display, lets the user
    /// identify which key they're looking at without exposing the
    /// secret half.
    pub key_prefix:   String,
    pub scopes:       Vec<String>,
    pub created_by:   Option<UserId>,
    pub created_at:   OffsetDateTime,
    pub last_used_at: Option<OffsetDateTime>,
    pub expires_at:   Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct NewApiKey {
    #[validate(length(min = 1, max = 80))]
    pub name: String,

    #[serde(default)]
    pub scopes: Vec<String>,

    /// Optional absolute expiry. Server validates it's in the future.
    #[serde(default)]
    pub expires_at: Option<OffsetDateTime>,
}

/// One-shot response returned by POST /v1/api-keys. The raw `token` is
/// the only chance the caller has to grab it — afterwards only the
/// prefix is queryable.
#[derive(Debug, Clone, Serialize)]
pub struct IssuedApiKey {
    pub key:   ApiKey,
    /// Full plaintext token in `rmp_<32 chars>` form.
    pub token: String,
}
