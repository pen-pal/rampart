//! On-call schedules — notification-channel rotations.
//!
//! A schedule is an ordered ring of channels plus a cadence. "Who is on
//! call at instant T" is pure arithmetic over the anchor and rotation
//! length — there are no per-shift rows to materialise. Escalation steps
//! reference a schedule by id and resolve it to the current on-call channel
//! when they fire (see [`crate::escalation::EscalationStep::schedule_ids`]).

use crate::ids::{NotificationId, OnCallScheduleId};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use validator::Validate;

/// Shortest rotation we allow — 5 minutes. Tighter than this is almost
/// always a misconfiguration that would thrash the pager.
pub const MIN_ROTATION_SECONDS: i64 = 300;
/// Longest rotation — 365 days.
pub const MAX_ROTATION_SECONDS: i64 = 31_536_000;
/// Cap on ring size; keeps the JSONB array small and the UI sane.
pub const MAX_PARTICIPANTS: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnCallSchedule {
    pub id: OnCallScheduleId,
    pub name: String,
    pub rotation_seconds: i64,
    /// The instant participant[0]'s first shift begins (UTC).
    pub anchor: OffsetDateTime,
    /// Notification channels in rotation order.
    pub participant_ids: Vec<NotificationId>,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct NewOnCallSchedule {
    #[validate(length(min = 1, max = 120))]
    pub name: String,
    pub rotation_seconds: i64,
    pub anchor: OffsetDateTime,
    pub participant_ids: Vec<NotificationId>,
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct UpdateOnCallSchedule {
    #[validate(length(min = 1, max = 120))]
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub rotation_seconds: Option<i64>,
    #[serde(default)]
    pub anchor: Option<OffsetDateTime>,
    #[serde(default)]
    pub participant_ids: Option<Vec<NotificationId>>,
}

/// Shared validation for create + update. Serde can't express "non-empty,
/// bounded ring + in-range cadence", so the route layer calls this.
pub fn validate_rotation(
    rotation_seconds: i64,
    participants: &[NotificationId],
) -> Result<(), String> {
    if participants.is_empty() {
        return Err("a schedule needs at least one participant".into());
    }
    if participants.len() > MAX_PARTICIPANTS {
        return Err(format!("at most {MAX_PARTICIPANTS} participants"));
    }
    if !(MIN_ROTATION_SECONDS..=MAX_ROTATION_SECONDS).contains(&rotation_seconds) {
        return Err(format!(
            "rotation must be {MIN_ROTATION_SECONDS}–{MAX_ROTATION_SECONDS}s"
        ));
    }
    Ok(())
}

/// Index of the participant on call at `at`. Euclidean division means
/// instants before the anchor wrap backward through the ring rather than
/// landing out of range, so a schedule is resolvable for any `at`. `None`
/// only for an empty ring or a non-positive cadence (a malformed row).
pub fn on_call_index(
    anchor: OffsetDateTime,
    rotation_seconds: i64,
    participant_count: usize,
    at: OffsetDateTime,
) -> Option<usize> {
    if participant_count == 0 || rotation_seconds <= 0 {
        return None;
    }
    let elapsed = (at - anchor).whole_seconds();
    let period = elapsed.div_euclid(rotation_seconds);
    let idx = period.rem_euclid(participant_count as i64);
    Some(idx as usize)
}

/// The channel on call at `at`, or `None` for an empty/malformed schedule.
pub fn on_call_channel(schedule: &OnCallSchedule, at: OffsetDateTime) -> Option<NotificationId> {
    on_call_index(
        schedule.anchor,
        schedule.rotation_seconds,
        schedule.participant_ids.len(),
        at,
    )
    .and_then(|i| schedule.participant_ids.get(i).copied())
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::Duration;

    fn schedule(rotation_seconds: i64, n: usize) -> OnCallSchedule {
        OnCallSchedule {
            id: OnCallScheduleId::new(),
            name: "primary".into(),
            rotation_seconds,
            anchor: OffsetDateTime::UNIX_EPOCH,
            participant_ids: (0..n).map(|_| NotificationId::new()).collect(),
            created_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn rotates_each_period() {
        let weekly = 604_800;
        let s = schedule(weekly, 3);
        let at = |secs: i64| OffsetDateTime::UNIX_EPOCH + Duration::seconds(secs);

        // At the anchor and partway through the first shift → participant 0.
        assert_eq!(on_call_channel(&s, at(0)), Some(s.participant_ids[0]));
        assert_eq!(on_call_channel(&s, at(weekly - 1)), Some(s.participant_ids[0]));
        // Each subsequent week advances one slot.
        assert_eq!(on_call_channel(&s, at(weekly)), Some(s.participant_ids[1]));
        assert_eq!(on_call_channel(&s, at(2 * weekly)), Some(s.participant_ids[2]));
        // Ring wraps after the last participant.
        assert_eq!(on_call_channel(&s, at(3 * weekly)), Some(s.participant_ids[0]));
    }

    #[test]
    fn before_anchor_wraps_backward() {
        let s = schedule(100, 3);
        let at = |secs: i64| OffsetDateTime::UNIX_EPOCH + Duration::seconds(secs);
        // One period before the anchor is the participant just before [0].
        assert_eq!(on_call_channel(&s, at(-1)), Some(s.participant_ids[2]));
        assert_eq!(on_call_channel(&s, at(-100)), Some(s.participant_ids[2]));
        assert_eq!(on_call_channel(&s, at(-101)), Some(s.participant_ids[1]));
    }

    #[test]
    fn single_participant_is_always_on_call() {
        let s = schedule(3600, 1);
        let at = |secs: i64| OffsetDateTime::UNIX_EPOCH + Duration::seconds(secs);
        assert_eq!(on_call_channel(&s, at(0)), Some(s.participant_ids[0]));
        assert_eq!(on_call_channel(&s, at(999_999)), Some(s.participant_ids[0]));
    }

    #[test]
    fn malformed_resolves_to_none() {
        let epoch = OffsetDateTime::UNIX_EPOCH;
        assert_eq!(on_call_index(epoch, 0, 3, epoch), None); // non-positive cadence
        assert_eq!(on_call_index(epoch, 100, 0, epoch), None); // empty ring
        let empty = schedule(100, 0);
        assert_eq!(on_call_channel(&empty, epoch), None);
    }

    #[test]
    fn validates_rotation() {
        let three: Vec<_> = (0..3).map(|_| NotificationId::new()).collect();
        assert!(validate_rotation(3600, &three).is_ok());
        assert!(validate_rotation(MIN_ROTATION_SECONDS, &three).is_ok());
        assert!(validate_rotation(MAX_ROTATION_SECONDS, &three).is_ok());

        assert!(validate_rotation(3600, &[]).is_err()); // empty ring
        assert!(validate_rotation(60, &three).is_err()); // too fast
        assert!(validate_rotation(MAX_ROTATION_SECONDS + 1, &three).is_err()); // too slow
        let too_many: Vec<_> = (0..=MAX_PARTICIPANTS).map(|_| NotificationId::new()).collect();
        assert!(validate_rotation(3600, &too_many).is_err());
    }
}
