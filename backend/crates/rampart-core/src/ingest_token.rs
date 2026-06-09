//! Inbound ingest tokens — page-scoped webhook credentials.
//!
//! An ingest token authorizes one external system (e.g. Prometheus
//! Alertmanager) to POST alerts to a single status page, where they become
//! incidents. Unlike a personal API key, the raw token is stored as-is and
//! re-shown to the admin: the operator has to paste the full URL (token
//! included) into the alerting system's config, and a hashed-only design
//! would make the value un-recoverable.
//!
//! Scope is deliberately narrow: a token can only create / resolve
//! incidents on its own `status_page_id`. It carries no user identity.

use crate::ids::{IngestTokenId, StatusPageId};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestToken {
    pub id: IngestTokenId,
    pub status_page_id: StatusPageId,
    /// The opaque URL credential. Safe to reveal to the page admin — it
    /// IS the thing they paste into their alerting system's config.
    pub token: String,
    pub label: Option<String>,
    /// Optional generic-receiver mapping (RFC 6901 JSON Pointers). `None`
    /// for tokens used only with the named vendor receivers; required by
    /// the `/ingest/generic/{token}` receiver. Stored as raw JSON and
    /// deserialized into [`IngestMapping`] at request time so a stored
    /// mapping that predates a schema change still loads.
    pub mapping: Option<serde_json::Value>,
    pub created_at: OffsetDateTime,
    pub last_used_at: Option<OffsetDateTime>,
}

/// Operator-configured mapping for the generic JSON-path webhook receiver.
///
/// Each `*_path` is an RFC 6901 JSON Pointer evaluated against the inbound
/// body with `serde_json::Value::pointer()`. Every field is optional and
/// degrades gracefully (see `0056_ingest_token_mapping.sql` and
/// `docs/INGEST.md` for the full semantics).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IngestMapping {
    /// Pointer to the firing/resolved discriminator value.
    #[serde(default)]
    pub action_path: Option<String>,
    /// Value at `action_path` that means "create an incident".
    #[serde(default)]
    pub action_firing_value: Option<String>,
    /// Value at `action_path` that means "resolve the matching incident".
    #[serde(default)]
    pub action_resolved_value: Option<String>,
    /// Pointer to the incident title.
    #[serde(default)]
    pub title_path: Option<String>,
    /// Pointer to the incident body/content.
    #[serde(default)]
    pub content_path: Option<String>,
    /// Pointer to the stable dedup key.
    #[serde(default)]
    pub dedup_path: Option<String>,
    /// Default incident style: `info` | `warning` | `danger` | `primary` |
    /// `success`. Defaults to `info` when absent or unrecognized.
    #[serde(default)]
    pub style: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewIngestToken {
    pub label: Option<String>,
    /// Optional generic-receiver mapping set at mint time.
    #[serde(default)]
    pub mapping: Option<serde_json::Value>,
}
