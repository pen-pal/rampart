//! Minimal 5-field cron expression support for push (cron-job) monitors.
//!
//! Hand-rolled on purpose: the popular cron crates all sit on `chrono`,
//! and pulling a second date-time stack into the workspace for "when was
//! this job last supposed to run" isn't worth it. Standard syntax only:
//!
//!   ┌──────── minute        0–59
//!   │ ┌────── hour          0–23
//!   │ │ ┌──── day of month  1–31
//!   │ │ │ ┌── month         1–12
//!   │ │ │ │ ┌ day of week   0–7 (0 and 7 = Sunday)
//!   * * * * *
//!
//! Each field accepts `*`, `N`, `A-B`, `*/S`, `A-B/S`, and comma lists.
//! Names (`JAN`, `MON`) are not supported — the dashboard stores what the
//! operator types, and numeric crontab syntax is the lingua franca.
//!
//! The vixie-cron day rule is honoured: when BOTH day-of-month and
//! day-of-week are restricted, a day matches if EITHER does.
//!
//! Everything is UTC, same as the rest of Rampart's scheduling.

use time::{Date, OffsetDateTime, Time};

/// Set-bits representation of one parsed field keeps matching branchless.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CronExpr {
    minutes: u64, // bits 0–59
    hours: u32,   // bits 0–23
    dom: u32,     // bits 1–31
    months: u16,  // bits 1–12
    dow: u8,      // bits 0–6, 0 = Sunday
    dom_star: bool,
    dow_star: bool,
}

impl CronExpr {
    /// Parse a 5-field expression. Returns None on anything malformed —
    /// callers treat an unparseable expression as "no schedule" and fall
    /// back to interval semantics, surfacing the problem in the UI rather
    /// than guessing.
    pub fn parse(s: &str) -> Option<CronExpr> {
        let fields: Vec<&str> = s.split_whitespace().collect();
        if fields.len() != 5 {
            return None;
        }
        let minutes = parse_field(fields[0], 0, 59)?;
        let hours = parse_field(fields[1], 0, 23)?;
        let dom = parse_field(fields[2], 1, 31)?;
        let months = parse_field(fields[3], 1, 12)?;
        // 7 is Sunday too: fold it onto bit 0.
        let mut dow = parse_field(fields[4], 0, 7)?;
        if dow & (1 << 7) != 0 {
            dow = (dow & !(1 << 7)) | 1;
        }
        Some(CronExpr {
            minutes: minutes as u64,
            hours: hours as u32,
            dom: dom as u32,
            months: months as u16,
            dow: dow as u8,
            dom_star: fields[2] == "*",
            dow_star: fields[4] == "*",
        })
    }

    fn day_matches(&self, date: Date) -> bool {
        let month_ok = self.months & (1 << (date.month() as u8)) != 0;
        if !month_ok {
            return false;
        }
        let dom_ok = self.dom & (1 << date.day()) != 0;
        let dow_ok = self.dow & (1 << date.weekday().number_days_from_sunday()) != 0;
        // vixie rule: both restricted → OR; otherwise AND (a star side
        // always matches anyway).
        if !self.dom_star && !self.dow_star {
            dom_ok || dow_ok
        } else {
            dom_ok && dow_ok
        }
    }

    /// Most recent scheduled occurrence at or before `now` (UTC), looking
    /// back at most ~366 days. Day-granular skipping keeps the worst case
    /// (an annual schedule) at a few hundred cheap bitmask checks.
    pub fn prev_occurrence(&self, now: OffsetDateTime) -> Option<OffsetDateTime> {
        let mut date = now.date();
        for day_back in 0..=366 {
            if self.day_matches(date) {
                let (h_limit, m_limit) = if day_back == 0 {
                    (now.hour(), now.minute())
                } else {
                    (23, 59)
                };
                for h in (0..=h_limit).rev() {
                    if self.hours & (1 << h) == 0 {
                        continue;
                    }
                    let m_top = if day_back == 0 && h == h_limit {
                        m_limit
                    } else {
                        59
                    };
                    for m in (0..=m_top).rev() {
                        if self.minutes & (1 << m) != 0 {
                            let t = Time::from_hms(h, m, 0).ok()?;
                            return Some(OffsetDateTime::new_utc(date, t));
                        }
                    }
                }
            }
            date = date.previous_day()?;
        }
        None
    }
}

/// Parse one cron field into a bitmask over [min, max].
fn parse_field(field: &str, min: u8, max: u8) -> Option<u64> {
    let mut bits: u64 = 0;
    for item in field.split(',') {
        let (range, step) = match item.split_once('/') {
            Some((r, s)) => (r, s.parse::<u8>().ok().filter(|&s| s > 0)?),
            None => (item, 1),
        };
        let (lo, hi) = if range == "*" {
            (min, max)
        } else if let Some((a, b)) = range.split_once('-') {
            let a = a.parse::<u8>().ok()?;
            let b = b.parse::<u8>().ok()?;
            if a > b {
                return None;
            }
            (a, b)
        } else {
            let v = range.parse::<u8>().ok()?;
            // bare value with a step ("5/15") is crontab-legal: start at
            // the value, run to max.
            if step > 1 {
                (v, max)
            } else {
                (v, v)
            }
        };
        if lo < min || hi > max {
            return None;
        }
        let mut v = lo;
        while v <= hi {
            bits |= 1 << v;
            v += step;
        }
    }
    if bits == 0 {
        None
    } else {
        Some(bits)
    }
}

/// Cron-job settings read from a push monitor's `config` JSONB. Absent or
/// unparseable → None, and the monitor keeps plain interval semantics.
#[derive(Debug, Clone, Copy)]
pub struct CronSchedule {
    pub expr: CronExpr,
    /// How long past the scheduled time a completion ping may arrive
    /// before the run counts as missed.
    pub grace_seconds: i64,
}

/// Default missed-run grace — generous enough for queue jitter and slow
/// jobs, tight enough that a stuck 5-minute cron pages within minutes.
pub const DEFAULT_CRON_GRACE_SECONDS: i64 = 300;

impl CronSchedule {
    pub fn from_config(config: &serde_json::Value) -> Option<CronSchedule> {
        let expr = CronExpr::parse(config.get("cron")?.as_str()?)?;
        let grace_seconds = config
            .get("cron_grace_seconds")
            .and_then(|v| v.as_i64())
            .filter(|&g| g >= 0)
            .unwrap_or(DEFAULT_CRON_GRACE_SECONDS);
        Some(CronSchedule {
            expr,
            grace_seconds,
        })
    }
}

/// Optional cap on a single run's wall-clock, read from `config`. When a
/// `state=run` ping is older than this and no completion followed, the
/// scheduler marks the run overdue.
pub fn max_run_seconds(config: &serde_json::Value) -> Option<i64> {
    config
        .get("max_run_seconds")
        .and_then(|v| v.as_i64())
        .filter(|&s| s > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    fn prev(expr: &str, now: OffsetDateTime) -> Option<OffsetDateTime> {
        CronExpr::parse(expr).unwrap().prev_occurrence(now)
    }

    #[test]
    fn every_minute() {
        assert_eq!(
            prev("* * * * *", datetime!(2026-06-11 10:30:45 UTC)),
            Some(datetime!(2026-06-11 10:30:00 UTC))
        );
    }

    #[test]
    fn hourly_on_the_quarter() {
        assert_eq!(
            prev("15 * * * *", datetime!(2026-06-11 10:10:00 UTC)),
            Some(datetime!(2026-06-11 09:15:00 UTC))
        );
        assert_eq!(
            prev("15 * * * *", datetime!(2026-06-11 10:15:00 UTC)),
            Some(datetime!(2026-06-11 10:15:00 UTC))
        );
    }

    #[test]
    fn nightly_crosses_midnight() {
        assert_eq!(
            prev("30 2 * * *", datetime!(2026-06-11 01:00:00 UTC)),
            Some(datetime!(2026-06-10 02:30:00 UTC))
        );
    }

    #[test]
    fn steps_ranges_lists() {
        // */15 → :00 :15 :30 :45
        assert_eq!(
            prev("*/15 * * * *", datetime!(2026-06-11 10:44:00 UTC)),
            Some(datetime!(2026-06-11 10:30:00 UTC))
        );
        // business hours range
        assert_eq!(
            prev("0 9-17 * * *", datetime!(2026-06-11 07:00:00 UTC)),
            Some(datetime!(2026-06-10 17:00:00 UTC))
        );
        // comma list
        assert_eq!(
            prev("0 6,18 * * *", datetime!(2026-06-11 12:00:00 UTC)),
            Some(datetime!(2026-06-11 06:00:00 UTC))
        );
    }

    #[test]
    fn weekly_and_dow_seven_is_sunday() {
        // 2026-06-07 was a Sunday.
        assert_eq!(
            prev("0 8 * * 0", datetime!(2026-06-11 10:00:00 UTC)),
            Some(datetime!(2026-06-07 08:00:00 UTC))
        );
        assert_eq!(
            prev("0 8 * * 7", datetime!(2026-06-11 10:00:00 UTC)),
            Some(datetime!(2026-06-07 08:00:00 UTC))
        );
    }

    #[test]
    fn monthly_and_vixie_or_rule() {
        // First of the month.
        assert_eq!(
            prev("0 0 1 * *", datetime!(2026-06-11 10:00:00 UTC)),
            Some(datetime!(2026-06-01 00:00:00 UTC))
        );
        // Both dom + dow restricted → OR: "the 15th or any Monday".
        // 2026-06-08 was a Monday, after 2026-05-15 — Monday wins.
        assert_eq!(
            prev("0 0 15 * 1", datetime!(2026-06-10 10:00:00 UTC)),
            Some(datetime!(2026-06-08 00:00:00 UTC))
        );
    }

    #[test]
    fn rejects_malformed() {
        for bad in [
            "",
            "* * * *",
            "* * * * * *",
            "60 * * * *",
            "* 24 * * *",
            "* * 0 * *",
            "* * * 13 *",
            "* * * * 8",
            "a * * * *",
            "*/0 * * * *",
            "5-2 * * * *",
        ] {
            assert!(CronExpr::parse(bad).is_none(), "should reject {bad:?}");
        }
    }

    #[test]
    fn schedule_from_config() {
        let cfg = serde_json::json!({ "cron": "*/5 * * * *", "cron_grace_seconds": 60 });
        let s = CronSchedule::from_config(&cfg).unwrap();
        assert_eq!(s.grace_seconds, 60);

        // Default grace.
        let cfg = serde_json::json!({ "cron": "0 * * * *" });
        assert_eq!(
            CronSchedule::from_config(&cfg).unwrap().grace_seconds,
            DEFAULT_CRON_GRACE_SECONDS
        );

        // No cron key / bad expr → None.
        assert!(CronSchedule::from_config(&serde_json::json!({})).is_none());
        assert!(CronSchedule::from_config(&serde_json::json!({ "cron": "nope" })).is_none());

        // max_run_seconds helper.
        assert_eq!(
            max_run_seconds(&serde_json::json!({ "max_run_seconds": 900 })),
            Some(900)
        );
        assert_eq!(max_run_seconds(&serde_json::json!({})), None);
        assert_eq!(
            max_run_seconds(&serde_json::json!({ "max_run_seconds": -5 })),
            None
        );
    }
}
