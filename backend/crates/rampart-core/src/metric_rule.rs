//! Threshold alert rules over ingested metrics.

use crate::ids::{EscalationPolicyId, MetricRuleId, NotificationId, OrgId};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use validator::Validate;

/// Comparison applied to the latest sample: `value <op> threshold`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleOp {
    Gt,
    Lt,
    Gte,
    Lte,
    /// Z-score anomaly: fire when `|value - mean| / stddev` exceeds the
    /// threshold (in σ) over a rolling baseline. Needs the baseline stats, so
    /// it's evaluated specially, not via [`RuleOp::breached`].
    Anomaly,
}

impl RuleOp {
    pub fn as_str(&self) -> &'static str {
        match self {
            RuleOp::Gt => "gt",
            RuleOp::Lt => "lt",
            RuleOp::Gte => "gte",
            RuleOp::Lte => "lte",
            RuleOp::Anomaly => "anomaly",
        }
    }

    /// Symbol for human-facing messages ("queue_depth = 42 > 40").
    pub fn symbol(&self) -> &'static str {
        match self {
            RuleOp::Gt => ">",
            RuleOp::Lt => "<",
            RuleOp::Gte => "≥",
            RuleOp::Lte => "≤",
            RuleOp::Anomaly => "σ",
        }
    }

    /// Unknown DB strings fall back to `gt` — the CHECK constraint makes
    /// that unreachable in practice; this just avoids a panic path.
    pub fn from_db(s: &str) -> Self {
        match s {
            "lt" => RuleOp::Lt,
            "gte" => RuleOp::Gte,
            "lte" => RuleOp::Lte,
            "anomaly" => RuleOp::Anomaly,
            _ => RuleOp::Gt,
        }
    }

    pub fn breached(&self, value: f64, threshold: f64) -> bool {
        match self {
            RuleOp::Gt => value > threshold,
            RuleOp::Lt => value < threshold,
            RuleOp::Gte => value >= threshold,
            RuleOp::Lte => value <= threshold,
            // Anomaly needs baseline stats; the evaluator handles it.
            RuleOp::Anomaly => false,
        }
    }

    pub fn is_anomaly(&self) -> bool {
        matches!(self, RuleOp::Anomaly)
    }

    /// Z-score breach for the anomaly op: `|value - mean| / stddev > sigma`.
    /// No deviation when stddev is ~0 (a flat series never alarms).
    pub fn anomaly_breached(value: f64, mean: f64, stddev: f64, sigma: f64) -> bool {
        stddev > f64::EPSILON && ((value - mean).abs() / stddev) > sigma
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricRule {
    pub id: MetricRuleId,
    /// Owning tenant — scopes the scheduler's series reads (Phase 5).
    pub org_id: OrgId,
    pub name: String,
    /// Metric name the rule watches.
    pub metric: String,
    /// Exact label set identifying the series (not a matcher).
    pub labels: serde_json::Value,
    pub op: RuleOp,
    pub threshold: f64,
    /// Sustain window: the breach must hold this long before firing.
    pub for_seconds: i32,
    pub enabled: bool,
    pub channel_ids: Vec<NotificationId>,
    pub escalation_policy_id: Option<EscalationPolicyId>,
    pub breach_since: Option<OffsetDateTime>,
    pub firing_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct NewMetricRule {
    #[validate(length(min = 1, max = 120))]
    pub name: String,
    #[validate(length(min = 1, max = 200))]
    pub metric: String,
    #[serde(default = "empty_labels")]
    pub labels: serde_json::Value,
    pub op: RuleOp,
    pub threshold: f64,
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

fn empty_labels() -> serde_json::Value {
    serde_json::json!({})
}
fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct UpdateMetricRule {
    #[validate(length(min = 1, max = 120))]
    #[serde(default)]
    pub name: Option<String>,
    #[validate(length(min = 1, max = 200))]
    #[serde(default)]
    pub metric: Option<String>,
    #[serde(default)]
    pub labels: Option<serde_json::Value>,
    #[serde(default)]
    pub op: Option<RuleOp>,
    #[serde(default)]
    pub threshold: Option<f64>,
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

/// What one evaluation tick decided for one rule — the pure half of the
/// state machine, separated so it can be tested without a database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleTransition {
    /// Nothing changed.
    None,
    /// Breach observed; sustain window starts (or continues) — no alert.
    Pending,
    /// Breach outlasted `for_seconds` → alert now.
    Fire,
    /// Sample back inside the threshold before the rule ever fired.
    ClearPending,
    /// Sample back inside the threshold while firing → recovery alert.
    Resolve,
}

/// Decide the transition for one rule given whether the latest (fresh)
/// sample breaches. `breached = false` also covers "no fresh sample" —
/// a silent series resolves rather than firing forever on stale data.
pub fn rule_transition(
    breach_since: Option<OffsetDateTime>,
    firing_at: Option<OffsetDateTime>,
    for_seconds: i32,
    breached: bool,
    now: OffsetDateTime,
) -> RuleTransition {
    match (breached, breach_since, firing_at) {
        (true, None, _) => {
            if for_seconds == 0 {
                RuleTransition::Fire
            } else {
                RuleTransition::Pending
            }
        }
        (true, Some(_), Some(_)) => RuleTransition::None, // already firing
        (true, Some(since), None) => {
            if (now - since).whole_seconds() >= for_seconds as i64 {
                RuleTransition::Fire
            } else {
                RuleTransition::None // pending, still inside the window
            }
        }
        (false, _, Some(_)) => RuleTransition::Resolve,
        (false, Some(_), None) => RuleTransition::ClearPending,
        (false, None, None) => RuleTransition::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    const NOW: OffsetDateTime = datetime!(2026-06-11 12:00:00 UTC);

    #[test]
    fn ops_compare() {
        assert!(RuleOp::Gt.breached(41.0, 40.0));
        assert!(!RuleOp::Gt.breached(40.0, 40.0));
        assert!(RuleOp::Gte.breached(40.0, 40.0));
        assert!(RuleOp::Lt.breached(1.0, 2.0));
        assert!(RuleOp::Lte.breached(2.0, 2.0));
        // Anomaly never breaches via the plain compare (evaluator handles it).
        assert!(!RuleOp::Anomaly.breached(999.0, 1.0));
    }

    #[test]
    fn anomaly_zscore() {
        // mean 10, stddev 2: a value 18 is 4σ out → breaches at 3σ, not at 5σ.
        assert!(RuleOp::anomaly_breached(18.0, 10.0, 2.0, 3.0));
        assert!(!RuleOp::anomaly_breached(18.0, 10.0, 2.0, 5.0));
        // Within band.
        assert!(!RuleOp::anomaly_breached(11.0, 10.0, 2.0, 3.0));
        // Flat series (stddev 0) never alarms.
        assert!(!RuleOp::anomaly_breached(100.0, 10.0, 0.0, 3.0));
        assert_eq!(RuleOp::from_db("anomaly"), RuleOp::Anomaly);
        assert!(RuleOp::Anomaly.is_anomaly());
    }

    #[test]
    fn fires_immediately_without_sustain_window() {
        assert_eq!(
            rule_transition(None, None, 0, true, NOW),
            RuleTransition::Fire
        );
    }

    #[test]
    fn sustain_window_delays_firing() {
        // First breached tick → pending.
        assert_eq!(
            rule_transition(None, None, 300, true, NOW),
            RuleTransition::Pending
        );
        // 2 minutes in → still waiting.
        let since = NOW - time::Duration::seconds(120);
        assert_eq!(
            rule_transition(Some(since), None, 300, true, NOW),
            RuleTransition::None
        );
        // Past the window → fire.
        let since = NOW - time::Duration::seconds(301);
        assert_eq!(
            rule_transition(Some(since), None, 300, true, NOW),
            RuleTransition::Fire
        );
    }

    #[test]
    fn recovery_paths() {
        let t = NOW - time::Duration::seconds(60);
        // Firing + back to normal → resolve (recovery alert).
        assert_eq!(
            rule_transition(Some(t), Some(t), 0, false, NOW),
            RuleTransition::Resolve
        );
        // Pending (never fired) + back to normal → silent clear.
        assert_eq!(
            rule_transition(Some(t), None, 300, false, NOW),
            RuleTransition::ClearPending
        );
        // Quiet rule stays quiet.
        assert_eq!(
            rule_transition(None, None, 0, false, NOW),
            RuleTransition::None
        );
        // Already firing + still breached → no repeat page.
        assert_eq!(
            rule_transition(Some(t), Some(t), 0, true, NOW),
            RuleTransition::None
        );
    }
}
