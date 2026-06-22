//! Service Level Objectives with error budgets and burn-rate alerting.
//!
//! An SLO names an objective (e.g. 99.9% over 30 days) over one of two
//! indicator kinds — monitor availability or a ratio of two metric counters —
//! and tracks the rolling *error budget*: the slice of allowed badness the
//! objective leaves you. The pure math here (budget consumed, burn rate) is
//! split from the database so it can be unit-tested without I/O; migration 0098
//! holds the schema.

use crate::ids::{EscalationPolicyId, MonitorId, NotificationId, OrgId, SloId};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use validator::Validate;

/// Where the success ratio comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SliKind {
    /// Up heartbeats / total heartbeats for a monitor over the window.
    Monitor,
    /// SUM(good_metric) / SUM(total_metric) over the window.
    Metric,
}

impl SliKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SliKind::Monitor => "monitor",
            SliKind::Metric => "metric",
        }
    }
    pub fn from_db(s: &str) -> Self {
        match s {
            "metric" => SliKind::Metric,
            _ => SliKind::Monitor,
        }
    }
}

/// A 1-hour burn rate at or above this exhausts a 30-day budget in ~2 days —
/// the Google SRE "fast burn" page threshold. Window-relative, so it works for
/// any objective window, not just 30 days.
pub const FAST_BURN_THRESHOLD: f64 = 14.4;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Slo {
    pub id: SloId,
    /// Owning tenant — scopes the scheduler's metric_samples reads (Phase 5).
    pub org_id: OrgId,
    pub name: String,
    pub description: String,
    pub sli_kind: SliKind,
    pub monitor_id: Option<MonitorId>,
    pub good_metric: Option<String>,
    pub total_metric: Option<String>,
    pub labels: serde_json::Value,
    pub objective_pct: f64,
    pub window_days: i32,
    pub enabled: bool,
    pub channel_ids: Vec<NotificationId>,
    pub escalation_policy_id: Option<EscalationPolicyId>,
    pub breaching_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct NewSlo {
    #[validate(length(min = 1, max = 120))]
    pub name: String,
    #[serde(default)]
    #[validate(length(max = 500))]
    pub description: String,
    pub sli_kind: SliKind,
    #[serde(default)]
    pub monitor_id: Option<MonitorId>,
    #[serde(default)]
    pub good_metric: Option<String>,
    #[serde(default)]
    pub total_metric: Option<String>,
    #[serde(default = "empty_labels")]
    pub labels: serde_json::Value,
    #[validate(range(min = 0.0001, max = 99.9999))]
    pub objective_pct: f64,
    #[validate(range(min = 1, max = 365))]
    #[serde(default = "default_window")]
    pub window_days: i32,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub channel_ids: Vec<NotificationId>,
    #[serde(default)]
    pub escalation_policy_id: Option<EscalationPolicyId>,
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct UpdateSlo {
    #[validate(length(min = 1, max = 120))]
    #[serde(default)]
    pub name: Option<String>,
    #[validate(length(max = 500))]
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub good_metric: Option<String>,
    #[serde(default)]
    pub total_metric: Option<String>,
    #[serde(default)]
    pub labels: Option<serde_json::Value>,
    #[validate(range(min = 0.0001, max = 99.9999))]
    #[serde(default)]
    pub objective_pct: Option<f64>,
    #[validate(range(min = 1, max = 365))]
    #[serde(default)]
    pub window_days: Option<i32>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub channel_ids: Option<Vec<NotificationId>>,
    #[serde(default)]
    pub escalation_policy_id: Option<EscalationPolicyId>,
}

fn empty_labels() -> serde_json::Value {
    serde_json::json!({})
}
fn default_window() -> i32 {
    30
}
fn default_enabled() -> bool {
    true
}

/// A point-in-time read of an SLO's health — the computed half, returned by
/// the API alongside the row and used by the scheduler to decide pages.
#[derive(Debug, Clone, Serialize)]
pub struct SloSnapshot {
    /// Achieved success ratio over the full window, 0–100, or `None` when the
    /// window held no data (no heartbeats / no samples).
    pub achieved_pct: Option<f64>,
    /// Percent of the error budget consumed (0 = none, 100 = exhausted, may
    /// exceed 100 when over budget). `None` mirrors `achieved_pct`.
    pub consumed_pct: Option<f64>,
    /// Budget left, `100 - consumed`, floored at 0 for display.
    pub remaining_pct: Option<f64>,
    /// 1-hour burn rate (multiples of the sustainable rate), or `None`.
    pub burn_rate_1h: Option<f64>,
    /// Whether the SLO is currently breaching its budget (exhausted or fast
    /// burning).
    pub breaching: bool,
}

/// Allowed bad fraction left by the objective: a 99.9% objective permits 0.1%
/// badness → 0.001.
pub fn error_budget_ratio(objective_pct: f64) -> f64 {
    ((100.0 - objective_pct) / 100.0).max(f64::EPSILON)
}

/// Percent of the error budget consumed by an achieved ratio. 0 when perfect,
/// 100 at exactly the objective, >100 when worse than the objective.
pub fn budget_consumed_pct(achieved_pct: f64, objective_pct: f64) -> f64 {
    let bad = (100.0 - achieved_pct).max(0.0) / 100.0;
    (bad / error_budget_ratio(objective_pct)) * 100.0
}

/// Burn rate = how many times faster than sustainable the budget is being
/// spent over the given (short) window. 1.0 means "on pace to exactly exhaust
/// the budget across the full window"; the fast-burn page fires above
/// [`FAST_BURN_THRESHOLD`].
pub fn burn_rate(achieved_pct: f64, objective_pct: f64) -> f64 {
    let bad = (100.0 - achieved_pct).max(0.0) / 100.0;
    bad / error_budget_ratio(objective_pct)
}

/// Build a snapshot from a full-window achieved ratio and an optional short
/// (1h) ratio. Either may be `None` (no data).
pub fn snapshot(
    achieved_pct: Option<f64>,
    achieved_1h: Option<f64>,
    objective_pct: f64,
) -> SloSnapshot {
    let consumed = achieved_pct.map(|a| budget_consumed_pct(a, objective_pct));
    let remaining = consumed.map(|c| (100.0 - c).max(0.0));
    let burn_1h = achieved_1h.map(|a| burn_rate(a, objective_pct));
    let exhausted = consumed.map(|c| c >= 100.0).unwrap_or(false);
    let fast = burn_1h.map(|b| b >= FAST_BURN_THRESHOLD).unwrap_or(false);
    SloSnapshot {
        achieved_pct,
        consumed_pct: consumed,
        remaining_pct: remaining,
        burn_rate_1h: burn_1h,
        breaching: exhausted || fast,
    }
}

/// What one evaluation tick decided for one SLO.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SloTransition {
    /// No change.
    None,
    /// Budget just went breaching → page now.
    Fire,
    /// Budget recovered → recovery notice.
    Resolve,
}

/// Pure transition: compare current breaching state against the stored
/// `breaching_at` de-dup marker.
pub fn slo_transition(breaching_at: Option<OffsetDateTime>, breaching: bool) -> SloTransition {
    match (breaching, breaching_at.is_some()) {
        (true, false) => SloTransition::Fire,
        (false, true) => SloTransition::Resolve,
        _ => SloTransition::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_math() {
        // 99.9% objective → 0.1% budget. Perfect run → 0% consumed.
        assert!((budget_consumed_pct(100.0, 99.9) - 0.0).abs() < 1e-9);
        // Exactly at objective → budget fully spent.
        assert!((budget_consumed_pct(99.9, 99.9) - 100.0).abs() < 1e-6);
        // Worse than objective → over budget.
        assert!(budget_consumed_pct(99.8, 99.9) > 100.0);
        // Halfway: 99.95% achieved against a 0.1% budget → 50% consumed.
        assert!((budget_consumed_pct(99.95, 99.9) - 50.0).abs() < 1e-6);
    }

    #[test]
    fn burn_rate_scales() {
        // Achieving the objective exactly = burn rate 1.0.
        assert!((burn_rate(99.9, 99.9) - 1.0).abs() < 1e-6);
        // 10x worse badness = 10x burn.
        assert!((burn_rate(99.0, 99.9) - 10.0).abs() < 1e-3);
    }

    #[test]
    fn snapshot_breaching_paths() {
        // Healthy: no breach.
        let s = snapshot(Some(99.99), Some(99.99), 99.9);
        assert!(!s.breaching);
        assert!(s.remaining_pct.unwrap() > 0.0);
        // Exhausted budget breaches even with a calm 1h window.
        let s = snapshot(Some(99.0), Some(100.0), 99.9);
        assert!(s.breaching);
        assert_eq!(s.remaining_pct, Some(0.0));
        // Fast burn breaches even with budget left over the full window.
        let s = snapshot(Some(99.95), Some(95.0), 99.9);
        assert!(s.breaching); // 1h burn ~50x ≫ 14.4
                              // No data → not breaching, all None.
        let s = snapshot(None, None, 99.9);
        assert!(!s.breaching);
        assert!(s.achieved_pct.is_none());
    }

    #[test]
    fn transitions() {
        assert_eq!(slo_transition(None, true), SloTransition::Fire);
        assert_eq!(
            slo_transition(Some(OffsetDateTime::now_utc()), false),
            SloTransition::Resolve
        );
        assert_eq!(slo_transition(None, false), SloTransition::None);
        assert_eq!(
            slo_transition(Some(OffsetDateTime::now_utc()), true),
            SloTransition::None
        );
    }
}
