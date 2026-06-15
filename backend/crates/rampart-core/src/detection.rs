//! SIEM-style log detection rules and their findings (migration 0090).
//!
//! A detection rule is a saved query over the log tier: an optional service
//! scope, a minimum severity, and an optional case-insensitive body regex
//! (Postgres `~*`). Each scheduler tick counts the **new** matching log records
//! since the rule last ran; when that count reaches the rule's `threshold` the
//! rule raises a [`DetectionFinding`] and notifies its channels.
//!
//! This is occurrence-based, not the sustained-breach state machine the metric
//! / telemetry rules use: a finding records "N matches happened in this window",
//! which is what a SOC analyst triages. The IO (matching + finding writes)
//! lives in `rampart_db::detection`; this module is the shared vocabulary.

use crate::ids::{DetectionFindingId, DetectionRuleId, EscalationPolicyId, NotificationId};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use validator::Validate;

/// Analyst-facing severity of a detection rule and the findings it raises.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectionSeverity {
    Low,
    #[default]
    Medium,
    High,
    Critical,
}

impl DetectionSeverity {
    pub fn as_str(&self) -> &'static str {
        match self {
            DetectionSeverity::Low => "low",
            DetectionSeverity::Medium => "medium",
            DetectionSeverity::High => "high",
            DetectionSeverity::Critical => "critical",
        }
    }

    /// Unknown DB strings fall back to `medium` — the CHECK constraint makes
    /// that unreachable; this just avoids a panic path.
    pub fn from_db(s: &str) -> Self {
        match s {
            "low" => DetectionSeverity::Low,
            "high" => DetectionSeverity::High,
            "critical" => DetectionSeverity::Critical,
            _ => DetectionSeverity::Medium,
        }
    }
}

/// A persisted detection rule over the log tier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionRule {
    pub id: DetectionRuleId,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub severity: DetectionSeverity,
    /// Service-name scope; empty = any service.
    pub service: String,
    /// Minimum OTLP severity number (0 = any).
    pub min_level: i16,
    /// Case-insensitive POSIX regex matched against the log body with Postgres
    /// `~*`; empty = match any body. Validated against Postgres on write.
    pub body_regex: String,
    /// Optional structured match: require `attributes->>attr_key = attr_val`.
    /// Empty `attr_key` = no attribute constraint.
    pub attr_key: String,
    pub attr_val: String,
    /// Raise a finding when at least this many new matches land in a tick.
    pub threshold: i32,
    /// How far back a tick looks when it has no prior checkpoint (seconds).
    pub window_seconds: i32,
    pub channel_ids: Vec<NotificationId>,
    pub escalation_policy_id: Option<EscalationPolicyId>,
    /// Watermark: the upper bound of the last evaluated window. The next tick
    /// counts matches with `ts > last_checked_at`, so findings never
    /// double-count across ticks.
    pub last_checked_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct NewDetectionRule {
    #[validate(length(min = 1, max = 120))]
    pub name: String,
    #[validate(length(max = 500))]
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub severity: DetectionSeverity,
    #[validate(length(max = 200))]
    #[serde(default)]
    pub service: String,
    #[validate(range(min = 0, max = 24))]
    #[serde(default)]
    pub min_level: i16,
    #[validate(length(max = 500))]
    #[serde(default)]
    pub body_regex: String,
    #[validate(length(max = 200))]
    #[serde(default)]
    pub attr_key: String,
    #[validate(length(max = 500))]
    #[serde(default)]
    pub attr_val: String,
    #[validate(range(min = 1, max = 100_000))]
    #[serde(default = "default_threshold")]
    pub threshold: i32,
    #[validate(range(min = 1, max = 86400))]
    #[serde(default = "default_window")]
    pub window_seconds: i32,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub channel_ids: Vec<NotificationId>,
    #[serde(default)]
    pub escalation_policy_id: Option<EscalationPolicyId>,
}

fn default_threshold() -> i32 {
    1
}
fn default_window() -> i32 {
    300
}
fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct UpdateDetectionRule {
    #[validate(length(min = 1, max = 120))]
    #[serde(default)]
    pub name: Option<String>,
    #[validate(length(max = 500))]
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub severity: Option<DetectionSeverity>,
    #[validate(length(max = 200))]
    #[serde(default)]
    pub service: Option<String>,
    #[validate(range(min = 0, max = 24))]
    #[serde(default)]
    pub min_level: Option<i16>,
    #[validate(length(max = 500))]
    #[serde(default)]
    pub body_regex: Option<String>,
    #[validate(length(max = 200))]
    #[serde(default)]
    pub attr_key: Option<String>,
    #[validate(length(max = 500))]
    #[serde(default)]
    pub attr_val: Option<String>,
    #[validate(range(min = 1, max = 100_000))]
    #[serde(default)]
    pub threshold: Option<i32>,
    #[validate(range(min = 1, max = 86400))]
    #[serde(default)]
    pub window_seconds: Option<i32>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub channel_ids: Option<Vec<NotificationId>>,
    #[serde(default)]
    pub escalation_policy_id: Option<EscalationPolicyId>,
}

/// One raised detection: a count of matches over a concrete window, with a
/// sample log line for the analyst. Acknowledgement clears it from the
/// "needs triage" view without deleting the audit record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionFinding {
    pub id: DetectionFindingId,
    pub rule_id: DetectionRuleId,
    pub rule_name: String,
    pub severity: DetectionSeverity,
    pub match_count: i64,
    pub sample: Option<String>,
    pub service: Option<String>,
    pub window_from: OffsetDateTime,
    pub window_to: OffsetDateTime,
    pub created_at: OffsetDateTime,
    pub acknowledged_at: Option<OffsetDateTime>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_db_roundtrip() {
        for s in [
            DetectionSeverity::Low,
            DetectionSeverity::Medium,
            DetectionSeverity::High,
            DetectionSeverity::Critical,
        ] {
            assert_eq!(DetectionSeverity::from_db(s.as_str()), s);
        }
        assert_eq!(DetectionSeverity::from_db("bogus"), DetectionSeverity::Medium);
    }
}
