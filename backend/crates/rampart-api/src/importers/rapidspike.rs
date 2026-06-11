//! RapidSpike monitor export importer.
//!
//! Reads a RapidSpike `GET /v1/status-monitors` JSON dump — operators
//! capture one with their own API key, save it to disk, hand the path
//! to `rampart-import rapidspike`. We don't reach out to the RapidSpike
//! API ourselves: importers are one-shot, offline tools by design (see
//! `docs/IMPORTERS.md` + `CONTRIBUTING.md` "Importers" bullet).
//!
//! ## Mapping
//!
//! RapidSpike wraps the listing under `items`. Each entry has a
//! `monitor_type` field that selects the probe family. See the table
//! in `docs/IMPORTERS.md`. Anything not in the table is skipped with a
//! `tracing::warn!` line so the operator can hand-port the unusual
//! monitors after the import.
//!
//! ## Field translation
//!
//! | RapidSpike                       | Rampart `NewMonitor`              |
//! | -------------------------------- | --------------------------------- |
//! | `name`                           | `name` (required)                 |
//! | `url`                            | `url`                             |
//! | `monitor_type`                   | (selects probe kind)              |
//! | `check_interval` (seconds)       | `interval_seconds` (clamped to `10..=86400`; default `60`) |
//! | `timeout_seconds` (seconds)      | `timeout_seconds` (clamped to `1..=600`; default `16`) |

use serde::Deserialize;
use serde_json::Value;
use tracing::warn;

use rampart_core::monitor::NewMonitor;
use rampart_core::MonitorKind;

pub use super::ImportError;
use super::{ImportPlan, MappedMonitor, SkippedMonitor};

/// The minimal shape of the top-level RapidSpike export. Real exports
/// carry many more fields per monitor; we deserialise into
/// `serde_json::Value` so we can pick out only the ones we need and
/// ignore the rest without listing every possible variant.
#[derive(Debug, Deserialize)]
struct Export {
    items: Vec<Value>,
}

/// Parse a RapidSpike export and map every recognisable entry onto a
/// `NewMonitor`. Returns the mapped list + a list of skipped monitors
/// (with reasons). Pure function — no I/O, no DB; the integration test
/// uses this directly without standing up Postgres.
pub fn parse_and_map(json: &str) -> Result<ImportPlan, ImportError> {
    let export: Export = serde_json::from_str(json)?;
    if export.items.is_empty() {
        return Err(ImportError::NoMonitors);
    }

    let mut plan = ImportPlan::default();
    for raw in export.items {
        match map_one(&raw) {
            Ok(m) => plan.mapped.push(m),
            Err(s) => {
                warn!(
                    source_name = %s.source_name,
                    source_kind = %s.source_kind,
                    reason = %s.reason,
                    "skip: unsupported rapidspike monitor",
                );
                plan.skipped.push(s);
            }
        }
    }
    Ok(plan)
}

/// Map a single RapidSpike monitor onto a Rampart `NewMonitor`.
/// Returns the mapped form or a `SkippedMonitor` describing why we
/// couldn't translate this one.
fn map_one(raw: &Value) -> Result<MappedMonitor, SkippedMonitor> {
    let monitor_type = string_field(raw, "monitor_type").unwrap_or_else(|| "<missing>".to_string());
    let source_kind = monitor_type.clone();
    let source_name = string_field(raw, "name").unwrap_or_else(|| "<unnamed>".to_string());

    let mapped_kind = match map_kind(&monitor_type) {
        Some(k) => k,
        None => {
            let reason = format!("unsupported rapidspike monitor_type `{source_kind}`");
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

    let url = string_field(raw, "url");
    let interval_seconds = numeric_field(raw, "check_interval")
        .unwrap_or(60)
        .clamp(10, 86400);
    let timeout_seconds = numeric_field(raw, "timeout_seconds")
        .unwrap_or(16)
        .clamp(1, 600);

    let new_monitor = NewMonitor {
        name: source_name.clone(),
        kind: mapped_kind,
        url,
        hostname: None,
        port: None,
        config: Value::Object(serde_json::Map::new()),
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
    };

    Ok(MappedMonitor {
        source_name,
        source_kind,
        mapped_kind,
        new_monitor,
    })
}

/// RapidSpike `monitor_type` -> Rampart probe kind. Returns `None` for
/// shapes Rampart has no equivalent for (unknown future kinds); those
/// get reported as skipped.
fn map_kind(monitor_type: &str) -> Option<MonitorKind> {
    let s = monitor_type.to_ascii_lowercase();
    let kind = match s.as_str() {
        "http_check" => MonitorKind::Http,
        "tcp_check" => MonitorKind::Tcp,
        "dns_check" => MonitorKind::Dns,
        "ping_check" => MonitorKind::Ping,
        _ => return None,
    };
    Some(kind)
}

/// Pull a string out of the JSON object. Accepts both `"foo"` and `null`
/// (returns `None` for null), and tolerates a missing key.
fn string_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

/// Pull a numeric-ish field out of the JSON. RapidSpike ships numbers
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
    fn maps_each_monitor_type_to_correct_kind() {
        let json = r#"{"items":[
            {"name":"HTTP1","monitor_type":"http_check","url":"https://a.example.com","check_interval":60,"timeout_seconds":15},
            {"name":"TCP1","monitor_type":"tcp_check","url":"db.example.com","check_interval":120,"timeout_seconds":10},
            {"name":"DNS1","monitor_type":"dns_check","url":"example.com","check_interval":300,"timeout_seconds":10},
            {"name":"PING1","monitor_type":"ping_check","url":"10.0.0.1","check_interval":300,"timeout_seconds":10}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped.len(), 4);
        assert_eq!(plan.mapped[0].mapped_kind, MonitorKind::Http);
        assert_eq!(plan.mapped[1].mapped_kind, MonitorKind::Tcp);
        assert_eq!(plan.mapped[2].mapped_kind, MonitorKind::Dns);
        assert_eq!(plan.mapped[3].mapped_kind, MonitorKind::Ping);
    }

    #[test]
    fn fields_translate_correctly() {
        let json = r#"{"items":[
            {"name":"My HTTP","monitor_type":"http_check","url":"https://x","check_interval":90,"timeout_seconds":12}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        let m = &plan.mapped[0];
        assert_eq!(m.new_monitor.name, "My HTTP");
        assert_eq!(m.new_monitor.url.as_deref(), Some("https://x"));
        assert_eq!(m.new_monitor.interval_seconds, 90);
        assert_eq!(m.new_monitor.timeout_seconds, 12);
    }

    #[test]
    fn skips_unknown_monitor_type() {
        let json = r#"{"items":[
            {"name":"Mystery","monitor_type":"browser_check","url":"https://x","check_interval":60}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert!(plan.mapped.is_empty());
        assert_eq!(plan.skipped.len(), 1);
        assert_eq!(plan.skipped[0].source_kind, "browser_check");
    }

    #[test]
    fn skips_missing_name() {
        let json = r#"{"items":[
            {"monitor_type":"http_check","url":"https://x","check_interval":60}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert!(plan.mapped.is_empty());
        assert_eq!(plan.skipped.len(), 1);
    }

    #[test]
    fn interval_clamped_to_validation_range() {
        let json = r#"{"items":[
            {"name":"fast","monitor_type":"http_check","url":"https://x","check_interval":5},
            {"name":"slow","monitor_type":"http_check","url":"https://y","check_interval":99999}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].new_monitor.interval_seconds, 10);
        assert_eq!(plan.mapped[1].new_monitor.interval_seconds, 86400);
    }

    #[test]
    fn missing_items_array_errors() {
        let json = r#"{"not_items":[]}"#;
        let err = parse_and_map(json).unwrap_err();
        assert!(matches!(
            err,
            ImportError::Parse(_) | ImportError::NoMonitors
        ));
    }
}
