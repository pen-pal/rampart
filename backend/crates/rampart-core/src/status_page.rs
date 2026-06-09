//! Public status pages.
//!
//! Two shapes:
//!
//! - [`StatusPage`] is the admin view — full row plus the attached
//!   monitor IDs. Returned by the protected `/v1/status-pages` routes.
//! - [`PublicStatusPage`] is the unauthenticated view served at
//!   `/v1/public/status-pages/:slug`. Carries enough information to
//!   render the page without leaking probe configuration.

use crate::ids::{MonitorId, StatusPageId};
use crate::incident::IncidentStyle;
use crate::monitor::MonitorStatus;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use validator::Validate;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusPage {
    pub id: StatusPageId,
    pub slug: String,
    pub title: String,
    pub description: Option<String>,
    pub theme: String,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    /// Monitors shown on this page, in display order. Populated by
    /// detail / list reads; create returns the freshly attached set.
    #[serde(default)]
    pub monitor_ids: Vec<MonitorId>,
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct NewStatusPage {
    /// URL-safe, lowercase, dash-separated. Mirrors the DB CHECK from
    /// migration 0001: 2-40 chars from `[a-z0-9-]`. We validate here
    /// for a friendlier 400 error before the round trip.
    #[validate(length(min = 2, max = 40), regex(path = *SLUG_RE))]
    pub slug: String,

    #[validate(length(min = 1, max = 120))]
    pub title: String,

    #[serde(default)]
    pub description: Option<String>,

    #[serde(default = "default_theme")]
    pub theme: String,

    #[serde(default)]
    pub monitor_ids: Vec<MonitorId>,
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct UpdateStatusPage {
    #[validate(length(min = 1, max = 120))]
    pub title: Option<String>,

    #[serde(default)]
    pub description: Option<Option<String>>,

    #[serde(default)]
    pub theme: Option<String>,

    /// When present, REPLACES the attached set. When absent, leaves it alone.
    #[serde(default)]
    pub monitor_ids: Option<Vec<MonitorId>>,
}

fn default_theme() -> String {
    "light".into()
}

/// Public, read-only projection. Fields here are deliberately tight —
/// no probe targets, no notification channel info.
#[derive(Debug, Clone, Serialize)]
pub struct PublicStatusPage {
    pub slug: String,
    pub title: String,
    pub description: Option<String>,
    pub theme: String,
    pub generated_at: OffsetDateTime,
    pub monitors: Vec<PublicStatusMonitor>,
    /// Active incidents (active = TRUE), most-recent first, each
    /// carrying its running updates oldest-first.
    pub incidents: Vec<PublicIncident>,
    /// Resolved incidents, newest-first, capped at 30. Powers the
    /// "Incident history" section on the public page so visitors can
    /// see what's happened over the past month without an operator
    /// having to dig the data out of the audit log.
    #[serde(default)]
    pub incident_history: Vec<PublicResolvedIncident>,
}

/// Slimmed projection used for the public history pane. Carries the
/// resolved-at timestamp + duration so a visitor can see at a glance
/// "this took 47 minutes to fix" without the API exposing every field
/// on the underlying incident row.
#[derive(Debug, Clone, Serialize)]
pub struct PublicResolvedIncident {
    pub title: String,
    pub content: String,
    pub style: IncidentStyle,
    pub created_at: OffsetDateTime,
    pub resolved_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize)]
pub struct PublicIncident {
    pub title: String,
    pub content: String,
    pub style: IncidentStyle,
    pub pinned: bool,
    pub created_at: OffsetDateTime,
    pub updates: Vec<PublicIncidentUpdate>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PublicIncidentUpdate {
    pub message: String,
    pub posted_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize)]
pub struct PublicStatusMonitor {
    pub name: String,
    pub current_status: MonitorStatus,
    /// Uptime percentage over the last 90 days, [0.0, 100.0]. Null if
    /// no heartbeats have been recorded yet.
    pub uptime_90d: Option<f32>,
    /// Average response latency in milliseconds over the trailing 24
    /// hours for `up` heartbeats only. Null if no recent successful
    /// heartbeats. Used by the public status page to render the
    /// "Avg 142 ms" hint next to each component.
    pub avg_latency_ms_24h: Option<f32>,
    /// 90-day daily uptime strip, oldest day first. Each char encodes
    /// one day:
    ///   'u' all up
    ///   'd' any down
    ///   'w' any warn (no down)
    ///   'm' only maintenance heartbeats
    ///   'n' no data
    /// Always exactly 90 characters. Rendered as the dense per-monitor
    /// timeline bar on the public status page.
    pub daily_status_90d: String,
}

// Compile the slug regex once. We mirror the Postgres CHECK constraint
// exactly so a slug accepted here always lands in the DB and vice versa.
use once_cell::sync::Lazy;
use regex::Regex;
static SLUG_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-z0-9-]{2,40}$").expect("status-page slug regex compiles"));
