//! Maintenance windows.
//!
//! A window is a `[start_at, end_at)` range. While "now" sits inside the
//! window AND the window is `active`, any monitor attached to it is
//! reported as Maintenance instead of Up/Down — the scheduler suppresses
//! both the probe and any notification fan-out.
//!
//! `start_at` / `end_at` define the *first* occurrence; the `recurrence`
//! field decides how that template repeats:
//!   * `None`                       — single-shot (default)
//!   * `Daily { until }`            — every 24h, time-of-day window
//!   * `Weekly { weekdays, until }` — Daily but gated by weekday set
//!
//! Recurrence rules use the time-of-day from start_at/end_at. If
//! `end < start` in time-of-day (e.g. 23:00 → 01:00) the window wraps
//! past midnight. For Weekly+wrap the *start* date's weekday is what
//! gates the wrapped tail — explicit and predictable beats clever.

use crate::ids::{MaintenanceId, MonitorId};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use validator::Validate;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Recurrence {
    #[default]
    None,
    Daily {
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            with = "time::serde::rfc3339::option"
        )]
        until: Option<OffsetDateTime>,
    },
    Weekly {
        /// 0 = Sunday, 6 = Saturday. Empty disables the window.
        weekdays: Vec<u8>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            with = "time::serde::rfc3339::option"
        )]
        until: Option<OffsetDateTime>,
    },
}

impl Recurrence {
    /// True iff `now` is inside an occurrence of the window described by
    /// `start_at` / `end_at` under this recurrence rule.
    pub fn contains(
        &self,
        start_at: OffsetDateTime,
        end_at: OffsetDateTime,
        now: OffsetDateTime,
    ) -> bool {
        match self {
            Recurrence::None => start_at <= now && now < end_at,
            Recurrence::Daily { until } => {
                if now < start_at {
                    return false;
                }
                if let Some(u) = until {
                    if now > *u {
                        return false;
                    }
                }
                in_time_of_day_window(start_at, end_at, now)
            }
            Recurrence::Weekly { weekdays, until } => {
                if now < start_at {
                    return false;
                }
                if let Some(u) = until {
                    if now > *u {
                        return false;
                    }
                }
                let wd = now.weekday().number_days_from_sunday();
                if !weekdays.contains(&wd) {
                    return false;
                }
                in_time_of_day_window(start_at, end_at, now)
            }
        }
    }
}

fn in_time_of_day_window(
    start_at: OffsetDateTime,
    end_at: OffsetDateTime,
    now: OffsetDateTime,
) -> bool {
    let s = start_at.time();
    let e = end_at.time();
    let n = now.time();
    if e > s {
        n >= s && n < e
    } else {
        // Wraps midnight: window is [s, 24h) ∪ [00, e).
        n >= s || n < e
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaintenanceWindow {
    pub id: MaintenanceId,
    pub name: String,
    pub description: Option<String>,
    pub start_at: OffsetDateTime,
    pub end_at: OffsetDateTime,
    pub active: bool,
    pub created_at: OffsetDateTime,
    /// Monitors covered by this window. Populated on detail reads;
    /// list endpoints may leave this empty for performance.
    #[serde(default)]
    pub monitor_ids: Vec<MonitorId>,
    #[serde(default)]
    pub recurrence: Recurrence,
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct NewMaintenanceWindow {
    #[validate(length(min = 1, max = 120))]
    pub name: String,

    #[serde(default)]
    pub description: Option<String>,

    #[serde(with = "time::serde::rfc3339")]
    pub start_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub end_at: OffsetDateTime,

    /// Monitors to attach when creating the window. Can be empty —
    /// callers may attach later via the detail route.
    #[serde(default)]
    pub monitor_ids: Vec<MonitorId>,

    #[serde(default)]
    pub recurrence: Recurrence,
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn none_is_single_shot() {
        let s = datetime!(2026-05-27 10:00 UTC);
        let e = datetime!(2026-05-27 11:00 UTC);
        let r = Recurrence::None;
        assert!(r.contains(s, e, datetime!(2026-05-27 10:30 UTC)));
        assert!(!r.contains(s, e, datetime!(2026-05-28 10:30 UTC))); // next day
        assert!(!r.contains(s, e, datetime!(2026-05-27 09:59 UTC)));
    }

    #[test]
    fn daily_repeats_each_day() {
        let s = datetime!(2026-05-27 10:00 UTC);
        let e = datetime!(2026-05-27 11:00 UTC);
        let r = Recurrence::Daily { until: None };
        assert!(r.contains(s, e, datetime!(2026-05-27 10:30 UTC)));
        assert!(r.contains(s, e, datetime!(2026-05-28 10:30 UTC)));
        assert!(r.contains(s, e, datetime!(2026-06-30 10:30 UTC)));
        assert!(!r.contains(s, e, datetime!(2026-05-28 09:59 UTC)));
        // Before first occurrence — even Daily can't time-travel.
        assert!(!r.contains(s, e, datetime!(2026-05-26 10:30 UTC)));
    }

    #[test]
    fn daily_respects_until() {
        let s = datetime!(2026-05-27 10:00 UTC);
        let e = datetime!(2026-05-27 11:00 UTC);
        let r = Recurrence::Daily {
            until: Some(datetime!(2026-05-30 23:59 UTC)),
        };
        assert!(r.contains(s, e, datetime!(2026-05-30 10:30 UTC)));
        assert!(!r.contains(s, e, datetime!(2026-05-31 10:30 UTC)));
    }

    #[test]
    fn daily_wraps_midnight() {
        let s = datetime!(2026-05-27 23:00 UTC);
        let e = datetime!(2026-05-27 23:30 UTC); // sentinel; time portion only matters
        let e = e.replace_time(time::Time::from_hms(1, 0, 0).unwrap()); // 01:00
        let r = Recurrence::Daily { until: None };
        assert!(r.contains(s, e, datetime!(2026-05-28 00:30 UTC)));
        assert!(r.contains(s, e, datetime!(2026-05-27 23:30 UTC)));
        assert!(!r.contains(s, e, datetime!(2026-05-27 02:00 UTC)));
    }

    #[test]
    fn weekly_gates_by_weekday() {
        // start on a Wednesday (2026-05-27)
        let s = datetime!(2026-05-27 10:00 UTC);
        let e = datetime!(2026-05-27 11:00 UTC);
        let r = Recurrence::Weekly {
            weekdays: vec![3], // Wednesday only
            until: None,
        };
        assert!(r.contains(s, e, datetime!(2026-05-27 10:30 UTC))); // Wed
        assert!(!r.contains(s, e, datetime!(2026-05-28 10:30 UTC))); // Thu
        assert!(r.contains(s, e, datetime!(2026-06-03 10:30 UTC))); // next Wed
    }
}
