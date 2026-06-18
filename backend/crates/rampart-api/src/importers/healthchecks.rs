//! Healthchecks.io monitor export importer.
//!
//! Reads a Healthchecks.io `GET /api/v3/checks/` JSON dump — operators
//! capture one with their own read-only API key, save it to disk, hand
//! the path to `rampart-import healthchecks`. We don't reach out to the
//! Healthchecks.io API ourselves: importers are one-shot, offline tools
//! by design (see `docs/IMPORTERS.md` + `CONTRIBUTING.md` "Importers"
//! bullet).
//!
//! ## Mapping
//!
//! Healthchecks.io is **heartbeat-only** — the entire product is shaped
//! around "ping me every N minutes / on this cron / on this calendar
//! expression and alert when you don't". Every check maps onto Rampart's
//! `Push` probe kind. There is no per-check kind discrimination; the
//! `kind` field on a Healthchecks check (`simple` / `cron` /
//! `oncalendar`) selects how the *schedule* is expressed, not the probe
//! family. See the table in `docs/IMPORTERS.md`.
//!
//! ## Field translation
//!
//! | Healthchecks                    | Rampart `NewMonitor`                      |
//! | ------------------------------- | ----------------------------------------- |
//! | `name`                          | `name` (required — defaults to `slug` then `uuid` when absent) |
//! | `uuid`                          | `config["healthchecks_uuid"]` (for cross-referencing) |
//! | `kind`                          | `config["healthchecks_kind"]` (`simple` / `cron` / `oncalendar`) |
//! | `schedule`                      | `config["schedule"]` (cron / oncalendar expression; absent for `simple`) |
//! | `tz`                            | `config["tz"]`                            |
//! | `timeout` (seconds)             | `interval_seconds` (clamped to `10..=86400`; default `60`) |
//! | `grace` (seconds)               | `timeout_seconds` (clamped to `1..=600`; default `16`) |
//!
//! Healthchecks's `timeout` is **the cadence the ping is expected on**
//! (i.e. "I should hear from this thing every N seconds") — exactly
//! what Rampart's `Push.interval_seconds` means. The `grace` field is
//! how long Healthchecks waits before flipping to down after the next
//! expected ping doesn't arrive; that maps onto Rampart's
//! `timeout_seconds`. Missing / unparseable falls back to the
//! `NewMonitor` serde defaults.

use serde::Deserialize;
use serde_json::{Map, Value};
use tracing::warn;

use rampart_core::monitor::NewMonitor;
use rampart_core::MonitorKind;

pub use super::ImportError;
use super::{ImportPlan, MappedMonitor, SkippedMonitor};

/// The minimal shape of the top-level Healthchecks.io export. Real
/// exports carry many more fields per check; we deserialise into
/// `serde_json::Value` so we can pick out only the ones we need and
/// ignore the rest without listing every possible variant.
#[derive(Debug, Deserialize)]
struct Export {
    checks: Vec<Value>,
}

/// Parse a Healthchecks.io export and map every entry onto a `NewMonitor`.
/// Healthchecks is heartbeat-only so every check maps to `Push` — the
/// skipped-list ends up empty in practice. The pipeline still threads
/// `SkippedMonitor` for symmetry with the other importers + so a
/// future Healthchecks API revision adding non-heartbeat kinds can be
/// surfaced without a structural change.
pub fn parse_and_map(json: &str) -> Result<ImportPlan, ImportError> {
    let export: Export = serde_json::from_str(json)?;
    if export.checks.is_empty() {
        return Err(ImportError::NoMonitors);
    }

    let mut plan = ImportPlan::default();
    for raw in export.checks {
        match map_one(&raw) {
            Ok(m) => plan.mapped.push(m),
            Err(s) => {
                warn!(
                    source_name = %s.source_name,
                    source_kind = %s.source_kind,
                    reason = %s.reason,
                    "skip: unsupported healthchecks check",
                );
                plan.skipped.push(s);
            }
        }
    }
    Ok(plan)
}

/// Map a single Healthchecks.io check object onto a Rampart `NewMonitor`.
/// Returns the mapped form or a `SkippedMonitor` describing why we
/// couldn't translate this one. The only thing we skip on is a missing
/// name / slug / uuid — we need *something* to put in `Monitor.name`,
/// and Healthchecks guarantees at least one of those is present.
fn map_one(raw: &Value) -> Result<MappedMonitor, SkippedMonitor> {
    // Healthchecks `kind` selects the schedule expression style, not
    // the probe family. We surface it in the skipped-list label so a
    // future API revision adding e.g. `oncalendar_strict` is visible.
    let hc_kind = string_field(raw, "kind").unwrap_or_else(|| "simple".to_string());
    let source_kind = hc_kind.clone();
    let source_name = pick_name(raw).unwrap_or_else(|| "<unnamed>".to_string());

    if source_name == "<unnamed>" {
        return Err(SkippedMonitor {
            source_name,
            source_kind,
            reason: "missing name / slug / uuid".into(),
        });
    }

    // Stash the Healthchecks-specific fields under `config` so the
    // operator can cross-reference back to the source dashboard, and
    // so a future Rampart probe revision can rebuild the schedule
    // expression verbatim if we ever grow cron-shaped Push.
    let mut config_obj = Map::new();
    if let Some(uuid) = string_field(raw, "uuid") {
        config_obj.insert("healthchecks_uuid".to_string(), Value::String(uuid));
    }
    config_obj.insert(
        "healthchecks_kind".to_string(),
        Value::String(hc_kind.clone()),
    );
    if let Some(sched) = string_field(raw, "schedule") {
        if !sched.is_empty() {
            config_obj.insert("schedule".to_string(), Value::String(sched));
        }
    }
    if let Some(tz) = string_field(raw, "tz") {
        if !tz.is_empty() {
            config_obj.insert("tz".to_string(), Value::String(tz));
        }
    }

    // Healthchecks `timeout` is the expected cadence — directly maps
    // to Rampart's `Push.interval_seconds`. `grace` is the post-expiry
    // window before flipping to down — that's `timeout_seconds`. The
    // `simple` kind is the documented timeout-based heartbeat; cron /
    // oncalendar kinds compute their own next-expected timestamp on the
    // Healthchecks side, but the operator-friendly fallback is to use
    // the field-level defaults so the imported monitor still ticks.
    let interval_seconds = numeric_field(raw, "timeout").unwrap_or(60).clamp(10, 86400);
    let timeout_seconds = numeric_field(raw, "grace").unwrap_or(16).clamp(1, 600);

    let new_monitor = NewMonitor {
        name: source_name.clone(),
        kind: MonitorKind::Push,
        url: None,
        hostname: None,
        port: None,
        config: Value::Object(config_obj),
        interval_seconds,
        timeout_seconds,
        max_retries: 0,
        retry_interval_sec: 60,
        resend_interval_sec: 0,
        upside_down: false,
        http_method: "GET".into(),
        http_body: None,
        http_headers: None,
        accepted_statuses: vec![200, 201, 202, 203, 204, 205, 206, 207, 208, 226],
        follow_redirect: true,
        ignore_tls: false,
        proxy_id: None,
        group_id: None,
        slo_target_pct: None,
        slo_window_days: None,
        agent_id: None,
        escalation_policy_id: None,
        check_cert: false,
        cert_expiry_days: 14,
    };

    Ok(MappedMonitor {
        source_name,
        source_kind,
        mapped_kind: MonitorKind::Push,
        new_monitor,
    })
}

/// Pick the best human-facing name available on the source check.
/// Healthchecks.io guarantees at least one of `name` / `slug` / `uuid`
/// is present; preferring `name` keeps the imported display matching
/// the operator's existing mental model, with the others as fallback
/// so we don't have to skip rows that the operator never explicitly
/// labelled.
fn pick_name(raw: &Value) -> Option<String> {
    if let Some(n) = string_field(raw, "name") {
        if !n.is_empty() {
            return Some(n);
        }
    }
    if let Some(s) = string_field(raw, "slug") {
        if !s.is_empty() {
            return Some(s);
        }
    }
    if let Some(u) = string_field(raw, "uuid") {
        if !u.is_empty() {
            return Some(format!("healthchecks-{u}"));
        }
    }
    None
}

/// Pull a string out of the JSON object. Accepts both `"foo"` and `null`
/// (returns `None` for null), and tolerates a missing key.
fn string_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

/// Pull a numeric-ish field out of the JSON. Healthchecks's documented
/// schema ships bare JSON numbers, but accept stringified numbers
/// defensively to match the leniency the sibling importers apply.
fn numeric_field(v: &Value, key: &str) -> Option<i32> {
    match v.get(key)? {
        Value::Number(n) => n.as_i64().map(|x| x as i32),
        Value::String(s) => s.trim().parse::<i32>().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_check_maps_to_push() {
        let json = r#"{"checks":[
            {"uuid":"u1","name":"DB backup","slug":"db-backup","kind":"simple",
             "timeout":3600,"grace":300},
            {"uuid":"u2","name":"Nightly sync","slug":"nightly","kind":"cron",
             "schedule":"0 2 * * *","tz":"UTC","timeout":86400,"grace":600},
            {"uuid":"u3","name":"Hourly metrics","slug":"hourly","kind":"oncalendar",
             "schedule":"hourly","tz":"UTC","timeout":3600,"grace":120}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped.len(), 3);
        assert!(plan.skipped.is_empty());
        for m in &plan.mapped {
            assert_eq!(m.mapped_kind, MonitorKind::Push);
        }
    }

    #[test]
    fn name_uuid_kind_and_schedule_land_in_config() {
        let json = r#"{"checks":[
            {"uuid":"abcd-1234","name":"DB backup","slug":"db","kind":"cron",
             "schedule":"0 2 * * *","tz":"UTC","timeout":3600,"grace":300}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        let m = &plan.mapped[0];
        assert_eq!(m.new_monitor.name, "DB backup");
        assert_eq!(m.new_monitor.config["healthchecks_uuid"], "abcd-1234");
        assert_eq!(m.new_monitor.config["healthchecks_kind"], "cron");
        assert_eq!(m.new_monitor.config["schedule"], "0 2 * * *");
        assert_eq!(m.new_monitor.config["tz"], "UTC");
    }

    #[test]
    fn timeout_maps_to_interval_grace_to_timeout() {
        let json = r#"{"checks":[
            {"uuid":"u1","name":"every 5m","kind":"simple","timeout":300,"grace":60}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].new_monitor.interval_seconds, 300);
        assert_eq!(plan.mapped[0].new_monitor.timeout_seconds, 60);
    }

    #[test]
    fn falls_back_to_slug_then_uuid_when_name_missing() {
        let json = r#"{"checks":[
            {"uuid":"u1","slug":"nightly-only","kind":"simple","timeout":60},
            {"uuid":"u2","kind":"simple","timeout":60}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].new_monitor.name, "nightly-only");
        assert_eq!(plan.mapped[1].new_monitor.name, "healthchecks-u2");
    }

    #[test]
    fn defaults_apply_when_timeout_and_grace_absent() {
        let json = r#"{"checks":[
            {"uuid":"u1","name":"defaults","kind":"simple"}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].new_monitor.interval_seconds, 60);
        assert_eq!(plan.mapped[0].new_monitor.timeout_seconds, 16);
    }

    #[test]
    fn skips_only_when_no_name_slug_or_uuid() {
        let json = r#"{"checks":[
            {"kind":"simple","timeout":60}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert!(plan.mapped.is_empty());
        assert_eq!(plan.skipped.len(), 1);
    }

    #[test]
    fn missing_checks_array_errors() {
        let json = r#"{"not_checks":[]}"#;
        let err = parse_and_map(json).unwrap_err();
        assert!(matches!(
            err,
            ImportError::Parse(_) | ImportError::NoMonitors
        ));
    }

    #[test]
    fn interval_clamped_to_validation_range() {
        let json = r#"{"checks":[
            {"uuid":"u1","name":"fast","kind":"simple","timeout":5},
            {"uuid":"u2","name":"slow","kind":"simple","timeout":99999}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].new_monitor.interval_seconds, 10);
        assert_eq!(plan.mapped[1].new_monitor.interval_seconds, 86400);
    }
}
