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
    /// Operator-set custom hostname (e.g. "status.acme.com"). The operator
    /// CNAMEs it to the Rampart host; we only store the canonical name.
    #[serde(default)]
    pub custom_domain: Option<String>,
    /// Brand logo — either an external URL or a `data:` URI for an uploaded
    /// image. Rendered to the left of the title on the public page.
    #[serde(default)]
    pub logo_url: Option<String>,
    /// Operator-authored CSS injected after the built-in stylesheet on the
    /// public page. NULL when unset.
    #[serde(default)]
    pub custom_css: Option<String>,
    /// Read-only flag: `true` when a `password_hash` is set on the row (the
    /// page is private). The hash itself is never serialized. Set by the db
    /// layer from `password_hash IS NOT NULL`.
    #[serde(default)]
    pub private: bool,
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

    /// Optional custom hostname. Shape-validated at the API edge (see
    /// `routes::status_pages`); uniqueness is enforced by the DB index.
    #[serde(default)]
    pub custom_domain: Option<String>,

    /// Optional brand logo — external URL or a `data:` URI.
    #[serde(default)]
    pub logo_url: Option<String>,

    /// Optional per-page custom CSS. Capped at 64 KB at the API edge.
    #[serde(default)]
    pub custom_css: Option<String>,

    /// Write-only plaintext password. When `Some`, the db layer hashes it
    /// with Argon2id and the page becomes private. Never serialized back
    /// out (this struct is `Deserialize`-only).
    #[serde(default)]
    pub password: Option<String>,

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

    /// `Some(Some(d))` sets the domain, `Some(None)` clears it, `None`
    /// leaves it alone. Double-Option so a clear is distinguishable from
    /// "field omitted".
    #[serde(default)]
    pub custom_domain: Option<Option<String>>,

    /// Same triple-state semantics as `custom_domain`.
    #[serde(default)]
    pub logo_url: Option<Option<String>>,

    /// Per-page custom CSS. Same triple-state semantics as `custom_domain`:
    /// `Some(Some(css))` sets it, `Some(None)` clears it, `None` leaves it.
    #[serde(default)]
    pub custom_css: Option<Option<String>>,

    /// Write-only plaintext password. `Some(Some(pw))` sets/changes the
    /// password (db layer hashes it → page becomes private), `Some(None)`
    /// clears it (page becomes public again), `None` leaves it unchanged.
    /// Never serialized back out.
    #[serde(default)]
    pub password: Option<Option<String>>,

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
    /// Canonical custom hostname, if the operator set one. The public
    /// view exposes it so the page knows its branded domain; actual
    /// routing is the operator's reverse proxy's job.
    #[serde(default)]
    pub custom_domain: Option<String>,
    /// Brand logo (external URL or `data:` URI). Rendered to the left of
    /// the page title.
    #[serde(default)]
    pub logo_url: Option<String>,
    /// Operator-authored CSS, injected after the built-in stylesheet on the
    /// public page. NULL when unset.
    #[serde(default)]
    pub custom_css: Option<String>,
    /// `true` when the page is password-protected. When the visitor hasn't
    /// proven the password yet, the rest of the payload is a locked stub:
    /// only `slug`/`title`/`private` carry real values, everything else is
    /// empty. The frontend keys off this flag to render a password prompt.
    #[serde(default)]
    pub private: bool,
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
    /// Scheduled-maintenance windows surfaced as a banner above the
    /// components list. Carries windows that are currently active OR
    /// start within the next 7 days AND are attached to at least one
    /// monitor shown on this page. Empty when nothing is scheduled.
    #[serde(default)]
    pub maintenance: Vec<PublicMaintenance>,
}

impl PublicStatusPage {
    /// Locked stub for a private page whose password hasn't been proven.
    /// Carries only the slug + title so the frontend can render a branded
    /// password prompt; everything that could leak component / incident
    /// detail is empty.
    pub fn locked(slug: String, title: String) -> Self {
        PublicStatusPage {
            slug,
            title,
            description: None,
            theme: default_theme(),
            custom_domain: None,
            logo_url: None,
            custom_css: None,
            private: true,
            generated_at: OffsetDateTime::now_utc(),
            monitors: Vec::new(),
            incidents: Vec::new(),
            incident_history: Vec::new(),
            maintenance: Vec::new(),
        }
    }
}

/// Public projection of a maintenance window. Carries just enough for
/// the public banner: the operator-authored title/description plus the
/// resolved occurrence window and whether it's live right now. The
/// `ends_at` is optional because a recurring window has no single end.
#[derive(Debug, Clone, Serialize)]
pub struct PublicMaintenance {
    pub title: String,
    pub description: Option<String>,
    pub starts_at: OffsetDateTime,
    pub ends_at: Option<OffsetDateTime>,
    /// True when `now` sits inside the window (per its recurrence rule).
    pub active: bool,
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
    /// 12-month uptime summary, oldest month first; today's month sits
    /// at the final index. Each chip rendered under the daily strip as
    /// "Jul 99.97%" / "Aug 100%" / etc. `uptime_pct` is None when no
    /// heartbeats were recorded that month so the UI can show a quiet
    /// "no data" chip instead of a misleading 0%.
    #[serde(default)]
    pub monthly_uptime_12mo: Vec<MonthlyUptimePoint>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MonthlyUptimePoint {
    /// First day of the month, UTC.
    pub year_month: time::Date,
    pub uptime_pct: Option<f32>,
}

// Compile the slug regex once. We mirror the Postgres CHECK constraint
// exactly so a slug accepted here always lands in the DB and vice versa.
use once_cell::sync::Lazy;
use regex::Regex;
static SLUG_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-z0-9-]{2,40}$").expect("status-page slug regex compiles"));
