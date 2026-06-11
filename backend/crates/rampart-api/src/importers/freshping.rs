//! Freshping (Freshworks) check importer.
//!
//! Reads a Freshping check export — operators capture one with their own
//! account, save it to disk, hand the path to `rampart-import
//! freshping`. We don't reach out to the Freshping API ourselves:
//! importers are one-shot, offline tools by design (see
//! `docs/IMPORTERS.md` + `CONTRIBUTING.md` "Importers" bullet).
//!
//! ## Mapping
//!
//! Each check carries a `check_type` constant that selects the probe
//! family:
//!
//! | Freshping `check_type` | Rampart `MonitorKind` |
//! | ---------------------- | --------------------- |
//! | `HTTP` / `HTTPS`       | `Http`                |
//! | `PING`                 | `Ping`                |
//! | `PORT`                 | `Tcp`                 |
//! | `DNS`                  | `Dns`                 |
//!
//! Anything else is skipped with a `tracing::warn!` line so the operator
//! can hand-port it.
//!
//! ## Field translation
//!
//! | Freshping        | Rampart `NewMonitor`                                            |
//! | ---------------- | --------------------------------------------------------------- |
//! | `name`           | `name` (required)                                               |
//! | `check_url`      | `url` for `HTTP`/`HTTPS`; `hostname` for `PING`/`PORT`/`DNS`     |
//! | `port`           | `port` (i32; dropped when `0` / absent)                         |
//! | `check_interval` | `interval_seconds` (already seconds)                            |

use serde::Deserialize;
use serde_json::Value;
use tracing::warn;

use rampart_core::monitor::NewMonitor;
use rampart_core::MonitorKind;

pub use super::ImportError;
use super::{ImportPlan, MappedMonitor, SkippedMonitor};

/// The minimal shape of the top-level Freshping checks export. Real
/// exports carry many more fields per check; we deserialise into
/// `serde_json::Value` so we can pick out only the ones we need and
/// ignore the rest.
#[derive(Debug, Deserialize)]
struct Export {
    checks: Vec<Value>,
}

/// Parse a Freshping checks export and map every recognisable entry onto
/// a `NewMonitor`. Returns the mapped list + a list of skipped checks
/// (with reasons). Pure function — no I/O, no DB; the integration test
/// uses this directly without standing up Postgres.
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
                    "skip: unsupported freshping check",
                );
                plan.skipped.push(s);
            }
        }
    }
    Ok(plan)
}

/// Map a single Freshping check onto a Rampart `NewMonitor`. Returns the
/// mapped form or a `SkippedMonitor` describing why we couldn't
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
                reason: format!("unsupported freshping check_type `{check_type}`"),
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

    let check_url = string_field(raw, "check_url").filter(|s| !s.is_empty());
    let port = numeric_field(raw, "port").filter(|p| *p > 0);

    // HTTP/HTTPS probes a full URL; everything else probes a host
    // (+ port for `PORT`).
    let (url, hostname) = match mapped_kind {
        MonitorKind::Http => (check_url, None),
        _ => (None, check_url),
    };

    // Freshping ships the cadence already in seconds.
    let interval_seconds = numeric_field(raw, "check_interval")
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
    };

    Ok(MappedMonitor {
        source_name,
        source_kind,
        mapped_kind,
        new_monitor,
    })
}

/// Freshping `check_type` -> Rampart probe kind. Returns `None` for
/// shapes Rampart has no equivalent for; those get reported as skipped.
/// Matched case-insensitively for defensiveness against API casing
/// drift, though the documented constants are upper-case.
fn map_kind(check_type: &str) -> Option<MonitorKind> {
    let kind = match check_type.trim().to_ascii_uppercase().as_str() {
        "HTTP" | "HTTPS" => MonitorKind::Http,
        "PING" => MonitorKind::Ping,
        "PORT" => MonitorKind::Tcp,
        "DNS" => MonitorKind::Dns,
        _ => return None,
    };
    Some(kind)
}

/// Pull a string out of the JSON object. Accepts both `"foo"` and `null`
/// (returns `None` for null), and tolerates a missing key.
fn string_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

/// Pull a numeric-ish field out of the JSON. Freshping ships numbers as
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
    fn maps_each_check_type_to_correct_kind() {
        let json = r#"{"checks":[
            {"id":1,"name":"web","check_url":"https://example.com","check_type":"HTTPS","check_interval":60},
            {"id":2,"name":"router","check_url":"10.0.0.1","check_type":"PING","check_interval":60},
            {"id":3,"name":"db","check_url":"db.example.com","check_type":"PORT","port":5432,"check_interval":60},
            {"id":4,"name":"resolver","check_url":"example.com","check_type":"DNS","check_interval":60}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped.len(), 4);
        assert_eq!(plan.mapped[0].mapped_kind, MonitorKind::Http);
        assert_eq!(plan.mapped[1].mapped_kind, MonitorKind::Ping);
        assert_eq!(plan.mapped[2].mapped_kind, MonitorKind::Tcp);
        assert_eq!(plan.mapped[3].mapped_kind, MonitorKind::Dns);
    }

    #[test]
    fn http_uses_check_url_as_url() {
        let json = r#"{"checks":[
            {"id":1,"name":"web","check_url":"https://example.com/health","check_type":"HTTP","check_interval":60}
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
    fn port_uses_check_url_as_hostname_and_port() {
        let json = r#"{"checks":[
            {"id":3,"name":"db","check_url":"db.example.com","check_type":"PORT","port":5432,"check_interval":120}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        let m = &plan.mapped[0];
        assert!(m.new_monitor.url.is_none());
        assert_eq!(m.new_monitor.hostname.as_deref(), Some("db.example.com"));
        assert_eq!(m.new_monitor.port, Some(5432));
        assert_eq!(m.new_monitor.interval_seconds, 120);
    }

    #[test]
    fn unknown_check_type_is_skipped() {
        let json = r#"{"checks":[
            {"id":9,"name":"flow","check_url":"https://x","check_type":"TRANSACTION","check_interval":60}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert!(plan.mapped.is_empty());
        assert_eq!(plan.skipped.len(), 1);
        assert_eq!(plan.skipped[0].source_kind, "TRANSACTION");
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
}
