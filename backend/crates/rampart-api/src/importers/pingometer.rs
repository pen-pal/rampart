//! Pingometer monitor importer.
//!
//! Reads a Pingometer monitor export — operators capture one with their
//! own account, save it to disk, hand the path to `rampart-import
//! pingometer`. We don't reach out to the Pingometer API ourselves:
//! importers are one-shot, offline tools by design (see
//! `docs/IMPORTERS.md` + `CONTRIBUTING.md` "Importers" bullet).
//!
//! ## Mapping
//!
//! Each monitor carries a `type` constant that selects the probe family:
//!
//! | Pingometer `type` | Rampart `MonitorKind` |
//! | ----------------- | --------------------- |
//! | `http`            | `Http`                |
//! | `ping`            | `Ping`                |
//! | `tcp`             | `Tcp`                 |
//!
//! Anything else is skipped with a `tracing::warn!` line so the operator
//! can hand-port it.
//!
//! ## Field translation
//!
//! | Pingometer | Rampart `NewMonitor`                                             |
//! | ---------- | --------------------------------------------------------------- |
//! | `name`     | `name` (required)                                               |
//! | `url`      | `url` for `http`; `hostname` for `ping`/`tcp`                   |
//! | `port`     | `port` (i32; `tcp`; dropped when `0` / absent)                  |
//! | `interval` | `interval_seconds` = `interval * 60` (MINUTES → seconds, floor 60) |

use serde::Deserialize;
use serde_json::Value;
use tracing::warn;

use rampart_core::monitor::NewMonitor;
use rampart_core::MonitorKind;

pub use super::ImportError;
use super::{ImportPlan, MappedMonitor, SkippedMonitor};

/// The minimal shape of the top-level Pingometer monitors export. Real
/// exports carry many more fields per monitor; we deserialise into
/// `serde_json::Value` so we can pick out only the ones we need and
/// ignore the rest.
#[derive(Debug, Deserialize)]
struct Export {
    monitors: Vec<Value>,
}

/// Parse a Pingometer monitors export and map every recognisable entry
/// onto a `NewMonitor`. Returns the mapped list + a list of skipped
/// monitors (with reasons). Pure function — no I/O, no DB; the
/// integration test uses this directly without standing up Postgres.
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
                    "skip: unsupported pingometer monitor",
                );
                plan.skipped.push(s);
            }
        }
    }
    Ok(plan)
}

/// Map a single Pingometer monitor onto a Rampart `NewMonitor`. Returns
/// the mapped form or a `SkippedMonitor` describing why we couldn't
/// translate this one.
fn map_one(raw: &Value) -> Result<MappedMonitor, SkippedMonitor> {
    let monitor_type = string_field(raw, "type").unwrap_or_else(|| "<missing>".to_string());
    let source_kind = monitor_type.clone();
    let source_name = string_field(raw, "name").unwrap_or_else(|| "<unnamed>".to_string());

    let mapped_kind = match map_kind(&monitor_type) {
        Some(k) => k,
        None => {
            return Err(SkippedMonitor {
                source_name,
                source_kind,
                reason: format!("unsupported pingometer type `{monitor_type}`"),
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

    let target = string_field(raw, "url").filter(|s| !s.is_empty());
    let port = numeric_field(raw, "port").filter(|p| *p > 0);

    // HTTP probes a full URL; ping/tcp probe a host (+ port for tcp).
    let (url, hostname) = match mapped_kind {
        MonitorKind::Http => (target, None),
        _ => (None, target),
    };

    // Pingometer ships the cadence as whole minutes; Rampart stores
    // seconds. Floor at 60s so a sub-minute interval never produces a
    // value below Rampart's minimum.
    let interval_minutes = numeric_field(raw, "interval").unwrap_or(1);
    let interval_seconds = (interval_minutes * 60).max(60);

    let new_monitor = NewMonitor {
        name: source_name.clone(),
        kind: mapped_kind,
        url,
        hostname,
        port,
        config: Value::Object(serde_json::Map::new()),
        interval_seconds,
        timeout_seconds: 16,
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
    };

    Ok(MappedMonitor {
        source_name,
        source_kind,
        mapped_kind,
        new_monitor,
    })
}

/// Pingometer `type` -> Rampart probe kind. Returns `None` for shapes
/// Rampart has no equivalent for; those get reported as skipped. Matched
/// case-insensitively for defensiveness against API casing drift.
fn map_kind(monitor_type: &str) -> Option<MonitorKind> {
    let kind = match monitor_type.trim().to_ascii_lowercase().as_str() {
        "http" => MonitorKind::Http,
        "ping" => MonitorKind::Ping,
        "tcp" => MonitorKind::Tcp,
        _ => return None,
    };
    Some(kind)
}

/// Pull a string out of the JSON object. Accepts both `"foo"` and `null`
/// (returns `None` for null), and tolerates a missing key.
fn string_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

/// Pull a numeric-ish field out of the JSON. Pingometer ships numbers as
/// bare JSON numbers in the documented schema, but accept stringified
/// numbers defensively.
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
    fn maps_each_type_to_correct_kind() {
        let json = r#"{"monitors":[
            {"name":"web","url":"https://example.com","type":"http","interval":1},
            {"name":"router","url":"10.0.0.1","type":"ping","interval":1},
            {"name":"db","url":"db.example.com","type":"tcp","port":5432,"interval":5}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped.len(), 3);
        assert_eq!(plan.mapped[0].mapped_kind, MonitorKind::Http);
        assert_eq!(plan.mapped[1].mapped_kind, MonitorKind::Ping);
        assert_eq!(plan.mapped[2].mapped_kind, MonitorKind::Tcp);
    }

    #[test]
    fn interval_minutes_become_seconds() {
        let json = r#"{"monitors":[
            {"name":"web","url":"https://example.com","type":"http","interval":5}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].new_monitor.interval_seconds, 300);
    }

    #[test]
    fn interval_floored_at_60() {
        let json = r#"{"monitors":[
            {"name":"web","url":"https://example.com","type":"http","interval":0}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].new_monitor.interval_seconds, 60);
    }

    #[test]
    fn tcp_uses_url_as_hostname_and_port() {
        let json = r#"{"monitors":[
            {"name":"db","url":"db.example.com","type":"tcp","port":5432,"interval":2}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        let m = &plan.mapped[0];
        assert!(m.new_monitor.url.is_none());
        assert_eq!(m.new_monitor.hostname.as_deref(), Some("db.example.com"));
        assert_eq!(m.new_monitor.port, Some(5432));
    }

    #[test]
    fn unknown_type_is_skipped() {
        let json = r#"{"monitors":[
            {"name":"flow","url":"https://x","type":"transaction","interval":1}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert!(plan.mapped.is_empty());
        assert_eq!(plan.skipped.len(), 1);
        assert_eq!(plan.skipped[0].source_kind, "transaction");
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
}
