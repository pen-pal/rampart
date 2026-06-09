//! Uptime.com check importer.
//!
//! Reads an Uptime.com `GET /api/v1/checks/` JSON dump — operators
//! capture one with their own API token, save it to disk, hand the path
//! to `rampart-import uptimecom`. We don't reach out to the Uptime.com
//! API ourselves: importers are one-shot, offline tools by design (see
//! `docs/IMPORTERS.md` + `CONTRIBUTING.md` "Importers" bullet).
//!
//! > **Not** updown.io — Uptime.com is a separate commercial product
//! > whose API wraps the check roster in a `{"results":[…]}` envelope and
//! > prefixes its monitor-config fields with `msp_`.
//!
//! ## Mapping
//!
//! Each check carries a `check_type` constant that selects the probe
//! family:
//!
//! | Uptime.com `check_type` | Rampart `MonitorKind` |
//! | ----------------------- | --------------------- |
//! | `HTTP`                  | `Http`                |
//! | `ICMP`                  | `Ping`                |
//! | `TCP`                   | `Tcp`                 |
//! | `DNS`                   | `Dns`                 |
//! | `SSL`                   | `Tls`                 |
//! | `SMTP`                  | `Smtp`                |
//!
//! Anything else (Uptime.com's transaction, API, group, malware, and
//! other higher-level check families) is skipped with a
//! `tracing::warn!` line so the operator can hand-port it.
//!
//! ## Field translation
//!
//! | Uptime.com               | Rampart `NewMonitor`                                          |
//! | ------------------------ | ------------------------------------------------------------- |
//! | `name`                   | `name` (required)                                             |
//! | `msp_address`            | `url` for `Http`; `hostname` for every other kind            |
//! | `msp_port`               | `port` (i32)                                                  |
//! | `msp_interval` (minutes) | `interval_seconds` = `msp_interval * 60` (floored to `60`)   |

use serde::Deserialize;
use serde_json::Value;
use tracing::warn;

use rampart_core::monitor::NewMonitor;
use rampart_core::MonitorKind;

pub use super::ImportError;
use super::{ImportPlan, MappedMonitor, SkippedMonitor};

/// The minimal shape of the top-level Uptime.com checks export. Real
/// exports carry many more fields per check; we deserialise into
/// `serde_json::Value` so we can pick out only the ones we need and
/// ignore the rest.
#[derive(Debug, Deserialize)]
struct Export {
    results: Vec<Value>,
}

/// Parse an Uptime.com checks export and map every recognisable entry
/// onto a `NewMonitor`. Returns the mapped list + a list of skipped
/// checks (with reasons). Pure function — no I/O, no DB; the integration
/// test uses this directly without standing up Postgres.
pub fn parse_and_map(json: &str) -> Result<ImportPlan, ImportError> {
    let export: Export = serde_json::from_str(json)?;
    if export.results.is_empty() {
        return Err(ImportError::NoMonitors);
    }

    let mut plan = ImportPlan::default();
    for raw in export.results {
        match map_one(&raw) {
            Ok(m) => plan.mapped.push(m),
            Err(s) => {
                warn!(
                    source_name = %s.source_name,
                    source_kind = %s.source_kind,
                    reason = %s.reason,
                    "skip: unsupported uptime.com check",
                );
                plan.skipped.push(s);
            }
        }
    }
    Ok(plan)
}

/// Map a single Uptime.com check onto a Rampart `NewMonitor`. Returns
/// the mapped form or a `SkippedMonitor` describing why we couldn't
/// translate this one.
fn map_one(raw: &Value) -> Result<MappedMonitor, SkippedMonitor> {
    let check_type = string_field(raw, "check_type").unwrap_or_else(|| "<missing>".to_string());
    let source_kind = check_type.clone();
    let source_name = string_field(raw, "name").unwrap_or_else(|| "<unnamed>".to_string());

    let mapped_kind = match map_kind(&check_type) {
        Some(k) => k,
        None => {
            return Err(SkippedMonitor {
                source_name,
                source_kind,
                reason: format!("unsupported uptime.com check_type `{check_type}`"),
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

    let address = string_field(raw, "msp_address").filter(|s| !s.is_empty());
    let port = numeric_field(raw, "msp_port").filter(|p| *p > 0);

    // HTTP probes a full URL; everything else probes a host (+ port).
    let (url, hostname) = match mapped_kind {
        MonitorKind::Http => (address, None),
        _ => (None, address),
    };

    // Uptime.com ships the cadence as whole minutes; Rampart stores
    // seconds. Floor at 60s so a sub-minute interval never produces a
    // value below Rampart's minimum.
    let interval_minutes = numeric_field(raw, "msp_interval").unwrap_or(1);
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
    };

    Ok(MappedMonitor {
        source_name,
        source_kind,
        mapped_kind,
        new_monitor,
    })
}

/// Uptime.com `check_type` -> Rampart probe kind. Returns `None` for
/// shapes Rampart has no equivalent for; those get reported as skipped.
/// Matched case-insensitively for defensiveness against API casing
/// drift, though the documented constants are upper-case.
fn map_kind(check_type: &str) -> Option<MonitorKind> {
    let kind = match check_type.trim().to_ascii_uppercase().as_str() {
        "HTTP" => MonitorKind::Http,
        "ICMP" => MonitorKind::Ping,
        "TCP" => MonitorKind::Tcp,
        "DNS" => MonitorKind::Dns,
        "SSL" => MonitorKind::Tls,
        "SMTP" => MonitorKind::Smtp,
        _ => return None,
    };
    Some(kind)
}

/// Pull a string out of the JSON object. Accepts both `"foo"` and `null`
/// (returns `None` for null), and tolerates a missing key.
fn string_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

/// Pull a numeric-ish field out of the JSON. Uptime.com ships numbers
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
    fn maps_each_check_type_to_correct_kind() {
        let json = r#"{"results":[
            {"pk":1,"name":"web","check_type":"HTTP","msp_address":"https://example.com","msp_interval":1},
            {"pk":2,"name":"router","check_type":"ICMP","msp_address":"10.0.0.1","msp_interval":1},
            {"pk":3,"name":"db","check_type":"TCP","msp_address":"db.example.com","msp_port":5432,"msp_interval":1},
            {"pk":4,"name":"resolver","check_type":"DNS","msp_address":"example.com","msp_interval":1},
            {"pk":5,"name":"cert","check_type":"SSL","msp_address":"example.com","msp_interval":1},
            {"pk":6,"name":"mail","check_type":"SMTP","msp_address":"smtp.example.com","msp_port":25,"msp_interval":1}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped.len(), 6);
        assert_eq!(plan.mapped[0].mapped_kind, MonitorKind::Http);
        assert_eq!(plan.mapped[1].mapped_kind, MonitorKind::Ping);
        assert_eq!(plan.mapped[2].mapped_kind, MonitorKind::Tcp);
        assert_eq!(plan.mapped[3].mapped_kind, MonitorKind::Dns);
        assert_eq!(plan.mapped[4].mapped_kind, MonitorKind::Tls);
        assert_eq!(plan.mapped[5].mapped_kind, MonitorKind::Smtp);
    }

    #[test]
    fn http_uses_address_as_url() {
        let json = r#"{"results":[
            {"pk":1,"name":"web","check_type":"HTTP","msp_address":"https://example.com/health","msp_interval":1}
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
    fn non_http_uses_address_as_hostname() {
        let json = r#"{"results":[
            {"pk":1,"name":"db","check_type":"TCP","msp_address":"db.example.com","msp_port":5432,"msp_interval":1}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        let m = &plan.mapped[0];
        assert!(m.new_monitor.url.is_none());
        assert_eq!(m.new_monitor.hostname.as_deref(), Some("db.example.com"));
        assert_eq!(m.new_monitor.port, Some(5432));
    }

    #[test]
    fn interval_minutes_convert_to_seconds() {
        let json = r#"{"results":[
            {"pk":1,"name":"a","check_type":"HTTP","msp_address":"https://a","msp_interval":5},
            {"pk":2,"name":"b","check_type":"HTTP","msp_address":"https://b","msp_interval":1}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].new_monitor.interval_seconds, 300);
        assert_eq!(plan.mapped[1].new_monitor.interval_seconds, 60);
    }

    #[test]
    fn interval_floored_to_60() {
        let json = r#"{"results":[
            {"pk":1,"name":"a","check_type":"HTTP","msp_address":"https://a","msp_interval":0}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].new_monitor.interval_seconds, 60);
    }

    #[test]
    fn unknown_check_type_is_skipped() {
        let json = r#"{"results":[
            {"pk":1,"name":"flow","check_type":"TRANSACTION","msp_address":"https://x","msp_interval":1}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert!(plan.mapped.is_empty());
        assert_eq!(plan.skipped.len(), 1);
        assert_eq!(plan.skipped[0].source_kind, "TRANSACTION");
    }

    #[test]
    fn missing_name_is_skipped() {
        let json = r#"{"results":[
            {"pk":1,"check_type":"HTTP","msp_address":"https://x","msp_interval":1}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert!(plan.mapped.is_empty());
        assert_eq!(plan.skipped.len(), 1);
    }

    #[test]
    fn missing_results_array_errors() {
        let json = r#"{"not_results":[]}"#;
        let err = parse_and_map(json).unwrap_err();
        assert!(matches!(
            err,
            ImportError::Parse(_) | ImportError::NoMonitors
        ));
    }
}
