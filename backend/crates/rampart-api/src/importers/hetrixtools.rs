//! HetrixTools uptime-monitor importer.
//!
//! Reads a HetrixTools `GET /v1/uptime-monitors` JSON dump — operators
//! capture one with their own API token, save it to disk, hand the path
//! to `rampart-import hetrixtools`. We don't reach out to the
//! HetrixTools API ourselves: importers are one-shot, offline tools by
//! design (see `docs/IMPORTERS.md` + `CONTRIBUTING.md` "Importers"
//! bullet).
//!
//! ## Mapping
//!
//! Each monitor carries a `Type` constant that selects the probe family:
//!
//! | HetrixTools `Type` | Rampart `MonitorKind` |
//! | ------------------ | --------------------- |
//! | `website`          | `Http`                |
//! | `service`          | `Tcp`                 |
//! | `ping`             | `Ping`                |
//!
//! Anything else is skipped with a `tracing::warn!` line so the operator
//! can hand-port it.
//!
//! ## Field translation
//!
//! | HetrixTools                | Rampart `NewMonitor`                                        |
//! | -------------------------- | ----------------------------------------------------------- |
//! | `Name`                     | `name` (required)                                           |
//! | `Target`                   | `url` for `website`; `hostname` for `service` / `ping`      |
//! | `Port`                     | `port` (i32; dropped when `0` / absent)                     |
//! | `Check_Frequency_Seconds`  | `interval_seconds`                                          |

use serde::Deserialize;
use serde_json::Value;
use tracing::warn;

use rampart_core::monitor::NewMonitor;
use rampart_core::MonitorKind;

pub use super::ImportError;
use super::{ImportPlan, MappedMonitor, SkippedMonitor};

/// The minimal shape of the top-level HetrixTools uptime-monitors
/// export. Real exports carry many more fields per monitor; we
/// deserialise into `serde_json::Value` so we can pick out only the
/// ones we need and ignore the rest.
#[derive(Debug, Deserialize)]
struct Export {
    monitors: Vec<Value>,
}

/// Parse a HetrixTools uptime-monitors export and map every recognisable
/// entry onto a `NewMonitor`. Returns the mapped list + a list of
/// skipped monitors (with reasons). Pure function — no I/O, no DB; the
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
                    "skip: unsupported hetrixtools monitor",
                );
                plan.skipped.push(s);
            }
        }
    }
    Ok(plan)
}

/// Map a single HetrixTools monitor onto a Rampart `NewMonitor`. Returns
/// the mapped form or a `SkippedMonitor` describing why we couldn't
/// translate this one.
fn map_one(raw: &Value) -> Result<MappedMonitor, SkippedMonitor> {
    let monitor_type = string_field(raw, "Type").unwrap_or_else(|| "<missing>".to_string());
    let source_kind = monitor_type.clone();
    let source_name = string_field(raw, "Name").unwrap_or_else(|| "<unnamed>".to_string());

    let mapped_kind = match map_kind(&monitor_type) {
        Some(k) => k,
        None => {
            return Err(SkippedMonitor {
                source_name,
                source_kind,
                reason: format!("unsupported hetrixtools type `{monitor_type}`"),
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

    let target = string_field(raw, "Target").filter(|s| !s.is_empty());
    let port = numeric_field(raw, "Port").filter(|p| *p > 0);

    // A `website` probes a full URL; `service` / `ping` probe a host
    // (+ port for `service`).
    let (url, hostname) = match mapped_kind {
        MonitorKind::Http => (target, None),
        _ => (None, target),
    };

    let interval_seconds = numeric_field(raw, "Check_Frequency_Seconds")
        .filter(|v| *v > 0)
        .unwrap_or(60);

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
        escalation_policy_id: None,
        check_cert: false,
        cert_expiry_days: 14,
    };

    Ok(MappedMonitor {
        source_name,
        source_kind,
        mapped_kind,
        new_monitor,
    })
}

/// HetrixTools `Type` -> Rampart probe kind. Returns `None` for shapes
/// Rampart has no equivalent for; those get reported as skipped. Matched
/// case-insensitively for defensiveness against API casing drift.
fn map_kind(monitor_type: &str) -> Option<MonitorKind> {
    let kind = match monitor_type.trim().to_ascii_lowercase().as_str() {
        "website" => MonitorKind::Http,
        "service" => MonitorKind::Tcp,
        "ping" => MonitorKind::Ping,
        _ => return None,
    };
    Some(kind)
}

/// Pull a string out of the JSON object. Accepts both `"foo"` and `null`
/// (returns `None` for null), and tolerates a missing key.
fn string_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

/// Pull a numeric-ish field out of the JSON. HetrixTools ships numbers
/// as bare JSON numbers in the documented schema, but accept
/// stringified numbers defensively.
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
            {"ID":"a","Name":"web","Target":"https://example.com","Type":"website","Check_Frequency_Seconds":60},
            {"ID":"b","Name":"db","Target":"db.example.com","Type":"service","Port":5432,"Check_Frequency_Seconds":60},
            {"ID":"c","Name":"router","Target":"10.0.0.1","Type":"ping","Check_Frequency_Seconds":60}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped.len(), 3);
        assert_eq!(plan.mapped[0].mapped_kind, MonitorKind::Http);
        assert_eq!(plan.mapped[1].mapped_kind, MonitorKind::Tcp);
        assert_eq!(plan.mapped[2].mapped_kind, MonitorKind::Ping);
    }

    #[test]
    fn website_uses_target_as_url() {
        let json = r#"{"monitors":[
            {"ID":"a","Name":"web","Target":"https://example.com/health","Type":"website","Check_Frequency_Seconds":60}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        let m = &plan.mapped[0];
        assert_eq!(
            m.new_monitor.url.as_deref(),
            Some("https://example.com/health")
        );
        assert!(m.new_monitor.hostname.is_none());
    }

    #[test]
    fn service_uses_target_as_hostname_and_port() {
        let json = r#"{"monitors":[
            {"ID":"b","Name":"db","Target":"db.example.com","Type":"service","Port":5432,"Check_Frequency_Seconds":120}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        let m = &plan.mapped[0];
        assert!(m.new_monitor.url.is_none());
        assert_eq!(m.new_monitor.hostname.as_deref(), Some("db.example.com"));
        assert_eq!(m.new_monitor.port, Some(5432));
        assert_eq!(m.new_monitor.interval_seconds, 120);
    }

    #[test]
    fn unknown_type_is_skipped() {
        let json = r#"{"monitors":[
            {"ID":"x","Name":"blob","Target":"x","Type":"smtp","Check_Frequency_Seconds":60}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert!(plan.mapped.is_empty());
        assert_eq!(plan.skipped.len(), 1);
        assert_eq!(plan.skipped[0].source_kind, "smtp");
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
