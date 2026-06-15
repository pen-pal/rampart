//! Threshold alert rules over the observability tiers (errors / traces / logs).
//!
//! These reuse the metric-rule state machine wholesale — [`RuleOp`] for the
//! comparison and [`rule_transition`](crate::metric_rule::rule_transition) for
//! the pending/firing/resolve logic plus the `for_seconds` sustain window. The
//! only thing specific to this module is *what value gets compared*: a windowed
//! aggregate over one tier, selected by [`TelemetryRuleKind`]. The IO that
//! computes that aggregate lives in `rampart_db::telemetry_rules`.

use crate::ids::{EscalationPolicyId, NotificationId, TelemetryRuleId};
use crate::metric_rule::RuleOp;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use validator::Validate;

/// Which tier an alert rule evaluates, and therefore how its observed value is
/// computed over the rolling window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryRuleKind {
    /// COUNT of error events in the window (optionally one project by name).
    ErrorRate,
    /// p95 span `duration_ms` in the window (optionally one service).
    TraceLatency,
    /// Percent of error spans (`status_code = 2`) in the window.
    TraceErrorRate,
    /// COUNT of log records at/above `min_level` in the window.
    LogVolume,
    /// SUM of profile `sample_count` in the window (optionally one service).
    /// A profiling-volume signal — e.g. CPU sampling spiking under load.
    ProfileSamples,
    /// p75 of RUM `lcp_ms` (Largest Contentful Paint) in the window, optionally
    /// one app. Alert when real-user load performance regresses.
    RumLcpP75,
}

impl TelemetryRuleKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            TelemetryRuleKind::ErrorRate => "error_rate",
            TelemetryRuleKind::TraceLatency => "trace_latency",
            TelemetryRuleKind::TraceErrorRate => "trace_error_rate",
            TelemetryRuleKind::LogVolume => "log_volume",
            TelemetryRuleKind::ProfileSamples => "profile_samples",
            TelemetryRuleKind::RumLcpP75 => "rum_lcp_p75",
        }
    }

    /// Unknown DB strings fall back to `error_rate` — the CHECK constraint
    /// makes that unreachable; this just avoids a panic path.
    pub fn from_db(s: &str) -> Self {
        match s {
            "trace_latency" => TelemetryRuleKind::TraceLatency,
            "trace_error_rate" => TelemetryRuleKind::TraceErrorRate,
            "log_volume" => TelemetryRuleKind::LogVolume,
            "profile_samples" => TelemetryRuleKind::ProfileSamples,
            "rum_lcp_p75" => TelemetryRuleKind::RumLcpP75,
            _ => TelemetryRuleKind::ErrorRate,
        }
    }

    /// Unit suffix for human-facing alert text ("p95 = 812 ms > 500").
    pub fn unit(&self) -> &'static str {
        match self {
            TelemetryRuleKind::ErrorRate => "events",
            TelemetryRuleKind::TraceLatency => "ms",
            TelemetryRuleKind::TraceErrorRate => "%",
            TelemetryRuleKind::LogVolume => "logs",
            TelemetryRuleKind::ProfileSamples => "samples",
            TelemetryRuleKind::RumLcpP75 => "ms",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryRule {
    pub id: TelemetryRuleId,
    pub name: String,
    pub kind: TelemetryRuleKind,
    /// Scope: project name (error_rate) or service name (trace/log kinds).
    /// Empty = all.
    pub target: String,
    /// Body substring filter, `log_volume` only. Empty = no filter.
    pub match_text: String,
    /// Minimum OTLP severity number, `log_volume` only (0 = any).
    pub min_level: i16,
    pub op: RuleOp,
    pub threshold: f64,
    /// Rolling evaluation window in seconds.
    pub window_seconds: i32,
    /// Sustain window: the breach must hold this long before firing.
    pub for_seconds: i32,
    pub enabled: bool,
    pub channel_ids: Vec<NotificationId>,
    /// Optional escalation policy — on fire, page its recipients too.
    pub escalation_policy_id: Option<EscalationPolicyId>,
    pub breach_since: Option<OffsetDateTime>,
    pub firing_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct NewTelemetryRule {
    #[validate(length(min = 1, max = 120))]
    pub name: String,
    pub kind: TelemetryRuleKind,
    #[validate(length(max = 200))]
    #[serde(default)]
    pub target: String,
    #[validate(length(max = 200))]
    #[serde(default)]
    pub match_text: String,
    #[validate(range(min = 0, max = 24))]
    #[serde(default)]
    pub min_level: i16,
    pub op: RuleOp,
    pub threshold: f64,
    #[validate(range(min = 1, max = 86400))]
    #[serde(default = "default_window")]
    pub window_seconds: i32,
    #[validate(range(min = 0, max = 86400))]
    #[serde(default)]
    pub for_seconds: i32,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub channel_ids: Vec<NotificationId>,
    #[serde(default)]
    pub escalation_policy_id: Option<EscalationPolicyId>,
}

fn default_window() -> i32 {
    300
}
fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct UpdateTelemetryRule {
    #[validate(length(min = 1, max = 120))]
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub kind: Option<TelemetryRuleKind>,
    #[validate(length(max = 200))]
    #[serde(default)]
    pub target: Option<String>,
    #[validate(length(max = 200))]
    #[serde(default)]
    pub match_text: Option<String>,
    #[validate(range(min = 0, max = 24))]
    #[serde(default)]
    pub min_level: Option<i16>,
    #[serde(default)]
    pub op: Option<RuleOp>,
    #[serde(default)]
    pub threshold: Option<f64>,
    #[validate(range(min = 1, max = 86400))]
    #[serde(default)]
    pub window_seconds: Option<i32>,
    #[validate(range(min = 0, max = 86400))]
    #[serde(default)]
    pub for_seconds: Option<i32>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub channel_ids: Option<Vec<NotificationId>>,
    #[serde(default)]
    pub escalation_policy_id: Option<EscalationPolicyId>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_db_roundtrip() {
        for k in [
            TelemetryRuleKind::ErrorRate,
            TelemetryRuleKind::TraceLatency,
            TelemetryRuleKind::TraceErrorRate,
            TelemetryRuleKind::LogVolume,
        ] {
            assert_eq!(TelemetryRuleKind::from_db(k.as_str()), k);
        }
    }

    #[test]
    fn unknown_kind_falls_back_to_error_rate() {
        assert_eq!(
            TelemetryRuleKind::from_db("nonsense"),
            TelemetryRuleKind::ErrorRate
        );
    }

    #[test]
    fn units_match_kind() {
        assert_eq!(TelemetryRuleKind::ErrorRate.unit(), "events");
        assert_eq!(TelemetryRuleKind::TraceLatency.unit(), "ms");
        assert_eq!(TelemetryRuleKind::TraceErrorRate.unit(), "%");
        assert_eq!(TelemetryRuleKind::LogVolume.unit(), "logs");
    }

    #[test]
    fn new_rule_deserializes_with_defaults() {
        let r: NewTelemetryRule = serde_json::from_value(serde_json::json!({
            "name": "spike",
            "kind": "error_rate",
            "op": "gt",
            "threshold": 10.0
        }))
        .unwrap();
        assert_eq!(r.window_seconds, 300);
        assert_eq!(r.for_seconds, 0);
        assert!(r.enabled);
        assert_eq!(r.target, "");
        assert_eq!(r.kind, TelemetryRuleKind::ErrorRate);
    }
}
