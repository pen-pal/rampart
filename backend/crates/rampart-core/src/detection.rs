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

use crate::ids::{DetectionFindingId, DetectionRuleId, EscalationPolicyId, NotificationId, OrgId};
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

/// A boolean condition tree over a log record — the Detection v2 matcher.
///
/// Replaces the flat AND-chain (service + min_level + body_regex + attr) with an
/// arbitrary `And` / `Or` / `Not` composition of leaf predicates, so a SOC can
/// express real detection content like `(service=auth AND body~/failed/) OR
/// (severity>=error AND NOT attr env=dev)`. Serialized to the `condition` JSONB
/// column; the DB layer compiles it to a parameterized SQL `WHERE`. Rules with a
/// null `condition` fall back to the legacy flat fields, so old rules keep
/// working unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DetectionCondition {
    /// All sub-conditions must match. Empty = matches everything.
    And { conditions: Vec<DetectionCondition> },
    /// Any sub-condition matches. Empty = matches nothing.
    Or { conditions: Vec<DetectionCondition> },
    /// Negate the inner condition.
    Not { condition: Box<DetectionCondition> },
    /// `service_name = value`.
    Service { value: String },
    /// `severity >= value` (OTLP severity number).
    MinLevel { value: i16 },
    /// `body ~* value` (case-insensitive POSIX regex).
    BodyRegex { value: String },
    /// `body ILIKE %value%` (case-insensitive substring).
    BodyContains { value: String },
    /// `attributes->>key = value`.
    Attr { key: String, value: String },
}

impl DetectionCondition {
    /// Total node count — bounded on write so a pathological tree can't generate
    /// unbounded SQL.
    pub fn node_count(&self) -> usize {
        1 + match self {
            DetectionCondition::And { conditions } | DetectionCondition::Or { conditions } => {
                conditions.iter().map(Self::node_count).sum()
            }
            DetectionCondition::Not { condition } => condition.node_count(),
            _ => 0,
        }
    }

    /// Nesting depth.
    pub fn depth(&self) -> usize {
        1 + match self {
            DetectionCondition::And { conditions } | DetectionCondition::Or { conditions } => {
                conditions.iter().map(Self::depth).max().unwrap_or(0)
            }
            DetectionCondition::Not { condition } => condition.depth(),
            _ => 0,
        }
    }

    /// Reject trees that are too large/deep or carry empty leaf values — keeps
    /// the generated SQL bounded and the rule meaningful.
    pub fn validate_tree(&self) -> Result<(), String> {
        if self.node_count() > 64 {
            return Err("condition has too many nodes (max 64)".into());
        }
        if self.depth() > 8 {
            return Err("condition nests too deeply (max 8)".into());
        }
        self.validate_leaves()
    }

    /// Collect every `BodyRegex` leaf value, so callers can validate each
    /// against the regex engine before the rule is stored.
    pub fn body_regexes<'a>(&'a self, out: &mut Vec<&'a str>) {
        match self {
            DetectionCondition::And { conditions } | DetectionCondition::Or { conditions } => {
                conditions.iter().for_each(|c| c.body_regexes(out));
            }
            DetectionCondition::Not { condition } => condition.body_regexes(out),
            DetectionCondition::BodyRegex { value } => out.push(value),
            _ => {}
        }
    }

    fn validate_leaves(&self) -> Result<(), String> {
        match self {
            DetectionCondition::And { conditions } | DetectionCondition::Or { conditions } => {
                conditions.iter().try_for_each(Self::validate_leaves)
            }
            DetectionCondition::Not { condition } => condition.validate_leaves(),
            DetectionCondition::Service { value }
            | DetectionCondition::BodyRegex { value }
            | DetectionCondition::BodyContains { value } => {
                if value.is_empty() {
                    Err("leaf predicate has an empty value".into())
                } else {
                    Ok(())
                }
            }
            DetectionCondition::Attr { key, value } => {
                if key.is_empty() || value.is_empty() {
                    Err("attr predicate needs both key and value".into())
                } else {
                    Ok(())
                }
            }
            DetectionCondition::MinLevel { .. } => Ok(()),
        }
    }
}

/// A persisted detection rule over the log tier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionRule {
    pub id: DetectionRuleId,
    /// Owning tenant — scopes the scheduler's log reads (Phase 5).
    pub org_id: OrgId,
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
    /// Detection v2 boolean condition tree. When set, it supersedes the flat
    /// `service`/`min_level`/`body_regex`/`attr_*` fields above; when null, the
    /// legacy flat match is used (so pre-v2 rules are unchanged).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<DetectionCondition>,
    /// Raise a finding when at least this many new matches land in a tick.
    pub threshold: i32,
    /// How far back a tick looks when it has no prior checkpoint (seconds).
    pub window_seconds: i32,
    /// Suppression window: after raising a finding, don't raise another for
    /// this rule until `cooldown_seconds` have elapsed (matches still advance
    /// the watermark, so they aren't re-counted later). `0` = no cooldown —
    /// raise on every tick that crosses the threshold (the legacy behavior).
    pub cooldown_seconds: i32,
    /// Per-entity aggregation key (a log attribute name). When non-empty, the
    /// tick groups matches by `attributes->>group_by` and raises one finding per
    /// entity whose count reaches `threshold` (e.g. group_by `user` + threshold
    /// 5 → fires once per user with ≥5 matches). Records lacking the attribute
    /// are ignored. Empty = aggregate the whole match set into one count.
    pub group_by: String,
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
    /// Detection v2 boolean condition tree (supersedes the flat fields above).
    #[serde(default)]
    pub condition: Option<DetectionCondition>,
    #[validate(range(min = 1, max = 100_000))]
    #[serde(default = "default_threshold")]
    pub threshold: i32,
    #[validate(range(min = 1, max = 86400))]
    #[serde(default = "default_window")]
    pub window_seconds: i32,
    /// See [`DetectionRule::cooldown_seconds`]. New rules default to a 5-minute
    /// suppression window so a noisy rule doesn't fire every tick out of the box.
    #[validate(range(min = 0, max = 86400))]
    #[serde(default = "default_cooldown")]
    pub cooldown_seconds: i32,
    /// Per-entity aggregation key (a log attribute name); empty = no grouping.
    #[validate(length(max = 200))]
    #[serde(default)]
    pub group_by: String,
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
fn default_cooldown() -> i32 {
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
    /// Detection v2 boolean condition tree. `Some` replaces the stored tree;
    /// `None` leaves it unchanged (use a clearing path if you need to drop it).
    #[serde(default)]
    pub condition: Option<DetectionCondition>,
    #[validate(range(min = 1, max = 100_000))]
    #[serde(default)]
    pub threshold: Option<i32>,
    #[validate(range(min = 1, max = 86400))]
    #[serde(default)]
    pub window_seconds: Option<i32>,
    #[validate(range(min = 0, max = 86400))]
    #[serde(default)]
    pub cooldown_seconds: Option<i32>,
    #[validate(length(max = 200))]
    #[serde(default)]
    pub group_by: Option<String>,
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
    /// For per-entity (group_by) findings: the entity value that crossed the
    /// threshold (e.g. the `user`/`src_ip`). `None` for whole-match-set findings.
    pub entity: Option<String>,
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
        assert_eq!(
            DetectionSeverity::from_db("bogus"),
            DetectionSeverity::Medium
        );
    }

    use DetectionCondition as C;

    fn sample_tree() -> DetectionCondition {
        C::Or {
            conditions: vec![
                C::And {
                    conditions: vec![
                        C::Service {
                            value: "auth".into(),
                        },
                        C::BodyRegex {
                            value: "fail".into(),
                        },
                    ],
                },
                C::Not {
                    condition: Box::new(C::Attr {
                        key: "env".into(),
                        value: "dev".into(),
                    }),
                },
            ],
        }
    }

    #[test]
    fn condition_node_count_and_depth() {
        let t = sample_tree();
        // Or + (And + Service + BodyRegex) + (Not + Attr) = 6 nodes.
        assert_eq!(t.node_count(), 6);
        // Or -> And -> Service = depth 3.
        assert_eq!(t.depth(), 3);
    }

    #[test]
    fn condition_serde_roundtrip() {
        let t = sample_tree();
        let json = serde_json::to_string(&t).unwrap();
        assert!(json.contains("\"type\":\"or\""));
        let back: DetectionCondition = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
    }

    #[test]
    fn condition_validate_rejects_empty_leaf_and_oversize() {
        assert!(sample_tree().validate_tree().is_ok());
        // Empty leaf value.
        assert!(C::Service {
            value: String::new()
        }
        .validate_tree()
        .is_err());
        // Attr missing a side.
        assert!(C::Attr {
            key: "k".into(),
            value: String::new()
        }
        .validate_tree()
        .is_err());
        // Too many nodes.
        let big = C::And {
            conditions: (0..70).map(|_| C::MinLevel { value: 1 }).collect(),
        };
        assert!(big.validate_tree().is_err());
    }

    #[test]
    fn condition_collects_body_regexes() {
        let t = sample_tree();
        let mut out = Vec::new();
        t.body_regexes(&mut out);
        assert_eq!(out, vec!["fail"]);
    }
}
