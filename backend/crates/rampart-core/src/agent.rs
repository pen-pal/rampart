//! Remote probe agents.
//!
//! An agent is a `rampart-agent` worker running outside this process —
//! another region, a private network segment — that probes the monitors
//! assigned to it and reports heartbeats back over the HTTP API. The
//! agent always dials out to the server (pull assignments, push results),
//! so it works behind NAT with no inbound connectivity.
//!
//! Auth mirrors api_keys: the bearer secret is `rmpa_<40 chars>`, only
//! its SHA-256 hash is stored, and the raw value is shown exactly once
//! in the [`IssuedAgent`] payload.

use crate::ids::{AgentId, MonitorId, OrgId};
use crate::MonitorStatus;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use validator::Validate;

/// How long after the last poll/report an agent still counts as online.
/// Agents poll every ~30s by default, so 90s tolerates two missed polls
/// before the dashboard flips the badge.
pub const ONLINE_GRACE_SECONDS: i64 = 90;

fn default_org_id() -> OrgId {
    OrgId::from_uuid(crate::org::DEFAULT_ORG_ID)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: AgentId,
    pub name: String,
    /// Free-form location label ("eu-west · Hetzner FSN"). Cosmetic.
    pub location: Option<String>,
    /// Agent build version, self-reported on each poll.
    pub version: Option<String>,
    pub last_seen_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
    /// Owning org (multi-tenancy). The agent wire-protocol metric push stamps
    /// telemetry with this. `serde(default)` keeps deserialization tolerant.
    #[serde(default = "default_org_id")]
    pub org_id: OrgId,
    /// Monitors currently assigned to this agent. Hydrated on list reads.
    #[serde(default)]
    pub monitor_count: i64,
    /// Derived from `last_seen_at` at read time so the dashboard doesn't
    /// have to duplicate the grace-window rule.
    #[serde(default)]
    pub online: bool,
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct NewAgent {
    #[validate(length(min = 1, max = 80))]
    pub name: String,
    #[validate(length(max = 120))]
    #[serde(default)]
    pub location: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct UpdateAgent {
    #[validate(length(min = 1, max = 80))]
    #[serde(default)]
    pub name: Option<String>,
    /// `Some("")` clears the location; `None` leaves it unchanged.
    #[validate(length(max = 120))]
    #[serde(default)]
    pub location: Option<String>,
}

/// One-shot response returned by POST /v1/agents. The raw `token` is the
/// only chance the caller has to grab it — afterwards only the hash lives
/// in the DB.
#[derive(Debug, Clone, Serialize)]
pub struct IssuedAgent {
    pub agent: Agent,
    /// Full plaintext token in `rmpa_<40 chars>` form.
    pub token: String,
}

/// One probe result reported by an agent via POST /v1/agent/heartbeats.
/// Mirrors the fields a local probe produces; the server stamps the
/// receive time itself rather than trusting an agent clock.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentResult {
    pub monitor_id: MonitorId,
    pub status: MonitorStatus,
    #[serde(default)]
    pub latency_ms: Option<i32>,
    #[serde(default)]
    pub status_code: Option<i32>,
    #[serde(default)]
    pub msg: Option<String>,
    #[serde(default)]
    pub retries: i32,
}
