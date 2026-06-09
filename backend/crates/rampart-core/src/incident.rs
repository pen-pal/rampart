//! Status-page incidents — communication, not investigation.
//!
//! Massively slimmed from Rampart v1. An incident here is a message
//! posted to a status page with optional running updates. No severity
//! beyond a visual `style`; no root cause, no AI summary, no post-mortem
//! document. If you want a write-up, write a blog post.

use crate::ids::{IncidentId, IncidentTemplateId, IncidentUpdateId, StatusPageId, UserId};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "incident_style", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum IncidentStyle {
    Info,
    Warning,
    Danger,
    Primary,
    Success,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Incident {
    pub id: IncidentId,
    pub status_page_id: StatusPageId,
    pub title: String,
    pub content: String,
    pub style: IncidentStyle,
    pub pinned: bool,
    pub active: bool,
    pub resolved_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
    pub created_by: Option<UserId>,
    /// Stable vendor-supplied key used by webhook ingest to match a
    /// "resolved" payload back to the incident it opened. `None` for
    /// manually created incidents.
    #[serde(default)]
    pub dedup_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentUpdate {
    pub id: IncidentUpdateId,
    pub incident_id: IncidentId,
    pub message: String,
    pub posted_at: OffsetDateTime,
    pub posted_by: Option<UserId>,
}

/// A reusable incident-update template. Global (not page-scoped): the
/// operator builds a small library of canned phases — "Investigating",
/// "Identified", "Monitoring", "Resolved" — and applies one into the
/// incident form instead of retyping. `style` reuses the incident colour
/// enum so applying a template fills both the body and the visual style.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentTemplate {
    pub id: IncidentTemplateId,
    pub name: String,
    pub body: String,
    pub style: IncidentStyle,
    pub created_at: OffsetDateTime,
}
