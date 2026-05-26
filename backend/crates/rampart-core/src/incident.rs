//! Status-page incidents — communication, not investigation.
//!
//! Massively slimmed from Rampart v1. An incident here is a message
//! posted to a status page with optional running updates. No severity
//! beyond a visual `style`; no root cause, no AI summary, no post-mortem
//! document. If you want a write-up, write a blog post.

use crate::ids::{IncidentId, IncidentUpdateId, StatusPageId, UserId};
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
    pub id:              IncidentId,
    pub status_page_id:  StatusPageId,
    pub title:           String,
    pub content:         String,
    pub style:           IncidentStyle,
    pub pinned:          bool,
    pub active:          bool,
    pub resolved_at:     Option<OffsetDateTime>,
    pub created_at:      OffsetDateTime,
    pub created_by:      Option<UserId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentUpdate {
    pub id:           IncidentUpdateId,
    pub incident_id:  IncidentId,
    pub message:      String,
    pub posted_at:    OffsetDateTime,
    pub posted_by:    Option<UserId>,
}
