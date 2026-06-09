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
    pub created_at: OffsetDateTime,
    pub last_used_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewIngestToken {
    pub label: Option<String>,
}
