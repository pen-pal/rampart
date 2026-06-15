//! Escalation policies — ordered notification ladders with acknowledge.

use crate::ids::{EscalationPolicyId, MonitorId, NotificationId, OnCallScheduleId, UserId};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use validator::Validate;

/// One rung of the ladder. `wait_seconds` is the delay after the
/// PREVIOUS step before this one fires (ignored on step 0, which fires
/// at episode open).
///
/// A step pages its fixed `channel_ids` plus the current on-call channel
/// of each schedule in `schedule_ids` (resolved when the step fires). At
/// least one of the two must be non-empty.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscalationStep {
    pub wait_seconds: i64,
    #[serde(default)]
    pub channel_ids: Vec<NotificationId>,
    /// On-call schedules paged at this step, each resolved to its current
    /// on-call channel at fire time. Additive to `channel_ids`.
    #[serde(default)]
    pub schedule_ids: Vec<OnCallScheduleId>,
}

pub const MAX_STEPS: usize = 10;
pub const MAX_STEP_WAIT_SECONDS: i64 = 86_400;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscalationPolicy {
    pub id: EscalationPolicyId,
    pub name: String,
    pub steps: Vec<EscalationStep>,
    pub created_at: OffsetDateTime,
    /// Monitors currently referencing this policy. Hydrated on list reads.
    #[serde(default)]
    pub monitor_count: i64,
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct NewEscalationPolicy {
    #[validate(length(min = 1, max = 120))]
    pub name: String,
    pub steps: Vec<EscalationStep>,
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct UpdateEscalationPolicy {
    #[validate(length(min = 1, max = 120))]
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub steps: Option<Vec<EscalationStep>>,
}

/// Shared step validation for create + update. Serde can't express
/// "non-empty, bounded, every step has channels", so the route layer
/// calls this explicitly.
pub fn validate_steps(steps: &[EscalationStep]) -> Result<(), String> {
    if steps.is_empty() {
        return Err("a policy needs at least one step".into());
    }
    if steps.len() > MAX_STEPS {
        return Err(format!("at most {MAX_STEPS} steps"));
    }
    for (i, s) in steps.iter().enumerate() {
        if s.channel_ids.is_empty() && s.schedule_ids.is_empty() {
            return Err(format!("step {} has no channels or schedules", i + 1));
        }
        if !(0..=MAX_STEP_WAIT_SECONDS).contains(&s.wait_seconds) {
            return Err(format!(
                "step {} wait must be 0–{MAX_STEP_WAIT_SECONDS}s",
                i + 1
            ));
        }
        // Step 0 fires at open; a wait there would silently delay the
        // first page, which nobody ever wants. Force it explicit.
        if i == 0 && s.wait_seconds != 0 {
            return Err("step 1 fires immediately — its wait must be 0".into());
        }
    }
    Ok(())
}

/// One open/closed escalation incident for a monitor.
#[derive(Debug, Clone, Serialize)]
pub struct EscalationEpisode {
    pub id: uuid::Uuid,
    /// Set for monitor episodes (kept for the FK cascade); None for rule
    /// subjects. Use `subject_kind`/`subject_ref` as the generic key.
    pub monitor_id: Option<MonitorId>,
    /// 'monitor' | 'telemetry_rule' (extensible).
    pub subject_kind: String,
    /// The subject identifier as text (monitor or rule id).
    pub subject_ref: String,
    pub policy_id: EscalationPolicyId,
    pub started_at: OffsetDateTime,
    pub last_step: i32,
    pub next_escalation_at: Option<OffsetDateTime>,
    pub acked_at: Option<OffsetDateTime>,
    pub acked_by: Option<UserId>,
    pub resolved_at: Option<OffsetDateTime>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(wait: i64, n: usize) -> EscalationStep {
        EscalationStep {
            wait_seconds: wait,
            channel_ids: (0..n).map(|_| NotificationId::new()).collect(),
            schedule_ids: vec![],
        }
    }

    #[test]
    fn validates_ladders() {
        assert!(validate_steps(&[step(0, 1)]).is_ok());
        assert!(validate_steps(&[step(0, 1), step(600, 2)]).is_ok());

        assert!(validate_steps(&[]).is_err());
        assert!(validate_steps(&[step(0, 0)]).is_err()); // no channels or schedules
        assert!(validate_steps(&[step(60, 1)]).is_err()); // step 0 must be immediate
        assert!(validate_steps(&[step(0, 1), step(-5, 1)]).is_err());
        assert!(validate_steps(&[step(0, 1), step(90_000, 1)]).is_err());
        let too_many: Vec<_> = (0..11)
            .map(|i| step(if i == 0 { 0 } else { 60 }, 1))
            .collect();
        assert!(validate_steps(&too_many).is_err());
    }

    #[test]
    fn step_with_only_a_schedule_is_valid() {
        let sched_step = EscalationStep {
            wait_seconds: 0,
            channel_ids: vec![],
            schedule_ids: vec![OnCallScheduleId::new()],
        };
        assert!(validate_steps(&[sched_step]).is_ok());
    }
}
