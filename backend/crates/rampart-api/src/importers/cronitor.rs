//! Cronitor monitor export importer.
//!
//! Reads a Cronitor `GET /api/monitors` JSON dump — operators capture
//! one with their own API key, save it to disk, hand the path to
//! `rampart-import cronitor`. We don't reach out to the Cronitor API
//! ourselves: importers are one-shot, offline tools by design (see
//! `docs/IMPORTERS.md` + `CONTRIBUTING.md` "Importers" bullet).
//!
//! ## Mapping
//!
//! Cronitor monitors come in four flavours: `heartbeat` and `job` (both
//! ping-shaped, mapping onto Rampart `Push`), `check` (HTTP check —
//! Rampart `Http`), and `uptime` (uptime ping — Rampart `Http`). See
//! the table in `docs/IMPORTERS.md`. Anything not in the table is
//! skipped with a `tracing::warn!` line so the operator can hand-port
//! the unusual monitors after the import.
//!
//! ## Field translation
//!
//! | Cronitor                          | Rampart `NewMonitor`              |
//! | --------------------------------- | --------------------------------- |
//! | `name`                            | `name` (required)                 |
//! | `code`                            | `config["cronitor_code"]`         |
//! | `request.url`                     | `url`                             |
//! | `request.method`                  | `http_method` (uppercased)        |
//! | `request.body`                    | `http_body`                       |
//! | `request.headers`                 | `http_headers`                    |
//! | `schedule`                        | `interval_seconds` (parsed when it matches a simple "every N seconds/minutes/hours" form; otherwise stashed in `config["schedule"]` and `interval_seconds` defaults to `60`) |
//! | `assertions`                      | `config["assertions"]` (verbatim — operator hand-ports the runtime semantics) |
//!
//! Cronitor's `schedule` is a free-form expression — it can be a cron
//! string ("0 * * * *"), an "every N units" phrase ("every 5 minutes"),
//! or a calendar shorthand ("hourly"). We try to parse the
//! `every N {seconds,minutes,hours}` form into a numeric interval; for
//! everything else (cron expressions, calendar shorthand) we keep the
//! original string in `config["schedule"]` and fall back to the
//! `NewMonitor` default interval of 60 seconds. Operators can rebuild
//! the cron-shaped scheduling by hand post-import.

use serde::Deserialize;
use serde_json::{Map, Value};
use tracing::warn;

use rampart_core::monitor::NewMonitor;
use rampart_core::MonitorKind;

pub use super::ImportError;
use super::{ImportPlan, MappedMonitor, SkippedMonitor};

/// The minimal shape of the top-level Cronitor export. Real exports
/// carry many more fields per monitor; we deserialise into
/// `serde_json::Value` so we can pick out only the ones we need and
/// ignore the rest without listing every possible variant.
#[derive(Debug, Deserialize)]
struct Export {
    monitors: Vec<Value>,
}

/// Parse a Cronitor export and map every recognisable entry onto a
/// `NewMonitor`. Returns the mapped list + a list of skipped monitors
/// (with reasons). Pure function — no I/O, no DB; the integration test
/// uses this directly without standing up Postgres.
pub fn parse_and_map(json: &str) -> Result<ImportPlan, ImportError> {
    let export: Export = serde_json::from_str(json)?;
    if export.monitors.is_empty() {
        return Err(ImportError::NoMonitors);
    }

    let mut plan = ImportPlan::default();
    for raw in export.monitors {
        match map_one(&raw) {
            Ok(m) => plan.mapped.push(m),
            Err(s) => {
                warn!(
                    source_name = %s.source_name,
                    source_kind = %s.source_kind,
                    reason = %s.reason,
                    "skip: unsupported cronitor monitor",
                );
                plan.skipped.push(s);
            }
        }
    }
    Ok(plan)
}

/// Map a single Cronitor monitor object onto a Rampart `NewMonitor`.
/// Returns the mapped form or a `SkippedMonitor` describing why we
/// couldn't translate this one.
fn map_one(raw: &Value) -> Result<MappedMonitor, SkippedMonitor> {
    let cronitor_type = string_field(raw, "type").unwrap_or_else(|| "<missing>".to_string());
    let source_kind = cronitor_type.clone();
    let source_name = string_field(raw, "name").unwrap_or_else(|| "<unnamed>".to_string());

    let mapped_kind = match map_kind(&cronitor_type) {
        Some(k) => k,
        None => {
            let reason = format!("unsupported cronitor monitor type `{source_kind}`");
            return Err(SkippedMonitor {
                source_name,
                source_kind,
                reason,
            });
        }
    };

    if source_name == "<unnamed>" {
        return Err(SkippedMonitor {
            source_name,
            source_kind,
            reason: "missing name".into(),
        });
    }

    let request = raw.get("request");
    // `url` is only meaningful for check / uptime kinds — heartbeat and
    // job monitors are Push and Rampart issues the webhook itself. We
    // still pull it defensively in case Cronitor surfaces a URL on
    // ping-shaped monitors for documentation purposes.
    let url = request.and_then(|r| string_field(r, "url"));
    let http_method = request
        .and_then(|r| string_field(r, "method"))
        .map(|m| m.trim().to_ascii_uppercase())
        .unwrap_or_else(|| "GET".into());
    let http_body = request
        .and_then(|r| string_field(r, "body"))
        .filter(|s| !s.is_empty());
    let http_headers = request
        .and_then(|r| r.get("headers").cloned())
        .and_then(|v| match &v {
            Value::Object(o) if o.is_empty() => None,
            Value::Array(a) if a.is_empty() => None,
            Value::Null => None,
            _ => Some(v),
        });

    // Push and Http both honour `interval_seconds`; for Push it's the
    // expected heartbeat cadence + Rampart's "alert when stale" timer,
    // for Http it's the polling cadence.
    let schedule = string_field(raw, "schedule");
    let interval_seconds = schedule
        .as_deref()
        .and_then(parse_simple_every)
        .unwrap_or(60)
        .clamp(10, 86400);

    let mut config_obj = Map::new();
    if let Some(code) = string_field(raw, "code") {
        config_obj.insert("cronitor_code".to_string(), Value::String(code));
    }
    if let Some(s) = schedule.as_deref() {
        if !s.is_empty() {
            config_obj.insert("schedule".to_string(), Value::String(s.to_string()));
        }
    }
    if let Some(asserts) = raw.get("assertions").cloned() {
        // Empty arrays carry no signal; keep the column tidy.
        let keep = match &asserts {
            Value::Array(a) => !a.is_empty(),
            _ => true,
        };
        if keep {
            config_obj.insert("assertions".to_string(), asserts);
        }
    }

    // For Push-family monitors, blank out the HTTP-shape fields so the
    // resulting row is unambiguous downstream — Rampart's Push probe
    // doesn't read `url` / `http_method` / `http_body` / `http_headers`,
    // and leaving them populated would confuse the operator at edit
    // time.
    let (url_out, method_out, body_out, headers_out) = match mapped_kind {
        MonitorKind::Push => (None, "GET".to_string(), None, None),
        _ => (url, http_method, http_body, http_headers),
    };

    let new_monitor = NewMonitor {
        name: source_name.clone(),
        kind: mapped_kind,
        url: url_out,
        hostname: None,
        port: None,
        config: Value::Object(config_obj),
        interval_seconds,
        timeout_seconds: 16,
        max_retries: 0,
        retry_interval_sec: 60,
        resend_interval_sec: 0,
        upside_down: false,
        http_method: method_out,
        http_body: body_out,
        http_headers: headers_out,
        accepted_statuses: vec![200, 201, 202, 203, 204, 205, 206, 207, 208, 226],
        follow_redirect: true,
        ignore_tls: false,
        proxy_id: None,
        group_id: None,
        slo_target_pct: None,
        slo_window_days: None,
    };

    Ok(MappedMonitor {
        source_name,
        source_kind,
        mapped_kind,
        new_monitor,
    })
}

/// Cronitor `type` -> Rampart probe kind. Returns `None` for shapes
/// Rampart has no equivalent for (unknown future kinds); those get
/// reported as skipped.
fn map_kind(cronitor_type: &str) -> Option<MonitorKind> {
    let s = cronitor_type.to_ascii_lowercase();
    let kind = match s.as_str() {
        // Cronitor's heartbeat + job monitors are both "ping me when
        // this thing runs / runs successfully" shaped — both map onto
        // Rampart's Push probe. The distinction (heartbeat = inbound
        // ping at cadence; job = cron-attached health) is preserved
        // in `config["cronitor_code"]` for operator reference.
        "heartbeat" | "job" => MonitorKind::Push,
        // `check` monitors are explicit HTTP checks. `uptime` is the
        // legacy uptime-ping family — also HTTP-shaped.
        "check" | "uptime" => MonitorKind::Http,
        _ => return None,
    };
    Some(kind)
}

/// Parse a simple "every N {seconds,minutes,hours,days}" Cronitor
/// schedule into a numeric second count. Returns `None` for cron
/// expressions, calendar shorthand ("hourly"), or anything else we
/// don't recognise — the caller falls back to the default 60s interval
/// in that case.
fn parse_simple_every(schedule: &str) -> Option<i32> {
    let s = schedule.trim().to_ascii_lowercase();
    let rest = s.strip_prefix("every ")?;
    let mut parts = rest.split_whitespace();
    let n: i64 = parts.next()?.parse().ok()?;
    let unit = parts.next()?;
    let seconds = match unit {
        "second" | "seconds" | "sec" | "secs" | "s" => n,
        "minute" | "minutes" | "min" | "mins" | "m" => n.checked_mul(60)?,
        "hour" | "hours" | "hr" | "hrs" | "h" => n.checked_mul(3600)?,
        "day" | "days" | "d" => n.checked_mul(86400)?,
        _ => return None,
    };
    i32::try_from(seconds).ok()
}

/// Pull a string out of the JSON object. Accepts both `"foo"` and `null`
/// (returns `None` for null), and tolerates a missing key.
fn string_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heartbeat_and_job_map_to_push() {
        let json = r#"{"monitors":[
            {"code":"abc1","name":"Nightly job","type":"heartbeat","schedule":"every 24 hours"},
            {"code":"abc2","name":"Backup runner","type":"job","schedule":"0 2 * * *"}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].mapped_kind, MonitorKind::Push);
        assert_eq!(plan.mapped[1].mapped_kind, MonitorKind::Push);
        // Push monitors don't carry URL / method even if the source did.
        assert!(plan.mapped[0].new_monitor.url.is_none());
    }

    #[test]
    fn check_and_uptime_map_to_http() {
        let json = r#"{"monitors":[
            {"code":"c1","name":"API check","type":"check",
             "request":{"url":"https://api.example.com/health","method":"GET"},
             "schedule":"every 60 seconds"},
            {"code":"c2","name":"Homepage","type":"uptime",
             "request":{"url":"https://www.example.com"},
             "schedule":"every 1 minute"}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].mapped_kind, MonitorKind::Http);
        assert_eq!(plan.mapped[1].mapped_kind, MonitorKind::Http);
        assert_eq!(
            plan.mapped[0].new_monitor.url.as_deref(),
            Some("https://api.example.com/health")
        );
        assert_eq!(plan.mapped[0].new_monitor.interval_seconds, 60);
        assert_eq!(plan.mapped[1].new_monitor.interval_seconds, 60);
    }

    #[test]
    fn cron_schedule_falls_back_to_default_but_kept_in_config() {
        let json = r#"{"monitors":[
            {"code":"c1","name":"Cron job","type":"check",
             "request":{"url":"https://x"},"schedule":"0 * * * *"}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].new_monitor.interval_seconds, 60);
        assert_eq!(plan.mapped[0].new_monitor.config["schedule"], "0 * * * *");
    }

    #[test]
    fn unknown_type_is_skipped() {
        let json = r#"{"monitors":[
            {"code":"c1","name":"Future kind","type":"synthetic"}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert!(plan.mapped.is_empty());
        assert_eq!(plan.skipped.len(), 1);
        assert_eq!(plan.skipped[0].source_kind, "synthetic");
    }

    #[test]
    fn parse_simple_every_handles_units() {
        assert_eq!(parse_simple_every("every 30 seconds"), Some(30));
        assert_eq!(parse_simple_every("every 5 minutes"), Some(300));
        assert_eq!(parse_simple_every("every 2 hours"), Some(7200));
        assert_eq!(parse_simple_every("every 1 day"), Some(86400));
        assert_eq!(parse_simple_every("EVERY 10 SECONDS"), Some(10));
        // Unrecognised forms return None — caller falls back to the
        // serde default.
        assert_eq!(parse_simple_every("hourly"), None);
        assert_eq!(parse_simple_every("0 * * * *"), None);
        assert_eq!(parse_simple_every(""), None);
    }

    #[test]
    fn interval_is_clamped() {
        // "every 5 seconds" -> 5 -> clamp to 10.
        let json = r#"{"monitors":[
            {"code":"c1","name":"fast","type":"check","request":{"url":"https://x"},
             "schedule":"every 5 seconds"}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].new_monitor.interval_seconds, 10);
    }

    #[test]
    fn code_and_assertions_land_in_config() {
        let json = r#"{"monitors":[
            {"code":"abc-123","name":"API","type":"check",
             "request":{"url":"https://x"},
             "assertions":[{"rule_type":"response_body","operator":"contains","value":"OK"}]}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(
            plan.mapped[0].new_monitor.config["cronitor_code"],
            "abc-123"
        );
        assert!(plan.mapped[0].new_monitor.config["assertions"].is_array());
    }

    #[test]
    fn missing_monitors_array_errors() {
        let json = r#"{"not_monitors":[]}"#;
        let err = parse_and_map(json).unwrap_err();
        assert!(matches!(
            err,
            ImportError::Parse(_) | ImportError::NoMonitors
        ));
    }

    #[test]
    fn check_method_is_uppercased() {
        let json = r#"{"monitors":[
            {"code":"c1","name":"POST check","type":"check",
             "request":{"url":"https://x","method":"post"}}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].new_monitor.http_method, "POST");
    }
}
