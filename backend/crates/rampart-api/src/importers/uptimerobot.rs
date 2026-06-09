//! UptimeRobot monitor export importer.
//!
//! Reads an UptimeRobot `POST /v2/getMonitors` JSON dump — operators
//! capture one with their own API key, save it to disk, hand the path
//! to `rampart-import uptimerobot`. We don't reach out to the
//! UptimeRobot API ourselves: importers are one-shot, offline tools by
//! design (see `docs/IMPORTERS.md` + `CONTRIBUTING.md` "Importers"
//! bullet).
//!
//! ## Mapping
//!
//! UptimeRobot's API shape is `(type, sub_type)` where `type` selects
//! the family (HTTP / Keyword / Ping / Port / Heartbeat) and `sub_type`
//! narrows the Port family into a specific protocol (HTTP / HTTPS /
//! FTP / SMTP / POP3 / IMAP / Custom). See the table in
//! `docs/IMPORTERS.md`. Anything not in the table is skipped with a
//! `tracing::warn!` line so the operator can hand-port the unusual
//! monitors after the import.
//!
//! ## Field translation
//!
//! | UptimeRobot                  | Rampart `NewMonitor`             |
//! | ---------------------------- | -------------------------------- |
//! | `friendly_name`              | `name`                           |
//! | `url`                        | `url` (types 1, 2)               |
//! | `url` (parsed host)          | `hostname` (type 4)              |
//! | `port`                       | `port` (i32, type 4)             |
//! | `keyword_value`              | `config["keyword"]` (type 2)     |
//! | `interval` (seconds)         | `interval_seconds`               |
//! | `timeout` (seconds)          | `timeout_seconds` (default 16)   |
//!
//! UptimeRobot's `interval` / `timeout` ship as bare JSON numbers per
//! the documented schema, but we accept stringified numbers defensively
//! (older API responses occasionally stringified them). Missing /
//! unparseable falls back to `NewMonitor`'s serde defaults (60s
//! interval, 16s timeout).

use serde::Deserialize;
use serde_json::{Map, Value};
use tracing::warn;

use rampart_core::monitor::NewMonitor;
use rampart_core::MonitorKind;

pub use super::ImportError;
use super::{ImportPlan, MappedMonitor, SkippedMonitor};

/// The minimal shape of the top-level UptimeRobot export. Real exports
/// carry many more fields per monitor; we deserialise into
/// `serde_json::Value` so we can pick out only the ones we need and
/// ignore the rest without listing every possible variant.
#[derive(Debug, Deserialize)]
struct Export {
    monitors: Vec<Value>,
}

/// Parse an UptimeRobot export and map every recognisable entry onto a
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
                    "skip: unsupported uptimerobot monitor",
                );
                plan.skipped.push(s);
            }
        }
    }
    Ok(plan)
}

/// Map a single UptimeRobot monitor object onto a Rampart `NewMonitor`.
/// Returns the mapped form or a `SkippedMonitor` describing why we
/// couldn't translate this one.
fn map_one(raw: &Value) -> Result<MappedMonitor, SkippedMonitor> {
    // UptimeRobot ships `type` / `sub_type` as bare ints. Surface both
    // in the skipped-list / log lines as a single "<type>/<sub_type>"
    // label so the operator can map the upstream shape that didn't fit.
    let type_code = numeric_field(raw, "type");
    let sub_type_code = numeric_field(raw, "sub_type");
    let source_kind = format_kind_label(type_code, sub_type_code);
    let source_name = string_field(raw, "friendly_name").unwrap_or_else(|| "<unnamed>".to_string());

    let mapped_kind = match map_kind(type_code, sub_type_code) {
        Some(k) => k,
        None => {
            let reason = format!("unsupported uptimerobot monitor `{source_kind}`");
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
            reason: "missing friendly_name".into(),
        });
    }
    let name = source_name.clone();

    // Endpoint addressing depends on the kind family. HTTP / Keyword
    // shapes carry a full URL; Port shapes carry a host (extracted from
    // the URL field) + a port number. Ping shapes carry only the URL
    // (UptimeRobot stores the target host there for ICMP probes).
    let raw_url = string_field(raw, "url");
    let (url, hostname, port) = endpoint_for(type_code, sub_type_code, raw_url.as_deref(), raw);

    // Keyword monitors carry the substring under `keyword_value`. Stash
    // it in `config["keyword"]` so the runtime probe has somewhere to
    // read it. Other kinds get an empty config object.
    let config = if type_code == Some(2) {
        let mut obj = Map::new();
        if let Some(kw) = string_field(raw, "keyword_value") {
            obj.insert("keyword".to_string(), Value::String(kw));
        }
        Value::Object(obj)
    } else {
        Value::Object(Map::new())
    };

    let interval_seconds = numeric_field(raw, "interval")
        .unwrap_or(60)
        .clamp(10, 86400);
    let timeout_seconds = numeric_field(raw, "timeout").unwrap_or(16).clamp(1, 600);

    let new_monitor = NewMonitor {
        name,
        kind: mapped_kind,
        url,
        hostname,
        port,
        config,
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
    };

    Ok(MappedMonitor {
        source_name,
        source_kind,
        mapped_kind,
        new_monitor,
    })
}

/// UptimeRobot `(type, sub_type)` -> Rampart probe kind. Returns `None`
/// for shapes Rampart has no equivalent for (unknown future type codes);
/// those get reported as skipped.
fn map_kind(type_code: Option<i32>, sub_type_code: Option<i32>) -> Option<MonitorKind> {
    let kind = match type_code? {
        1 => MonitorKind::Http,
        2 => MonitorKind::Keyword,
        3 => MonitorKind::Ping,
        4 => match sub_type_code.unwrap_or(99) {
            1 => MonitorKind::Tcp, // HTTP port 80
            2 => MonitorKind::Tcp, // HTTPS port 443
            3 => MonitorKind::Ftp,
            4 => MonitorKind::Smtp,
            5 => MonitorKind::Pop3,
            6 => MonitorKind::Imap,
            99 => MonitorKind::Tcp, // Custom — port carried explicitly
            _ => return None,
        },
        5 => MonitorKind::Push,
        _ => return None,
    };
    Some(kind)
}

/// Compose the (url, hostname, port) triple based on the kind family.
///
/// - Types 1 / 2 (HTTP / Keyword): preserve the full URL; no host/port.
/// - Type 3 (Ping): UptimeRobot stores the ICMP target in `url`; copy
///   it to `hostname` so Rampart's Ping probe addresses it correctly.
/// - Type 4 (Port): extract host from `url` (UptimeRobot's UI stores
///   the bare host there for port monitors), use the explicit `port`
///   field, or fall back to the sub_type's well-known port for
///   HTTP/HTTPS sub-types when `port` is absent.
/// - Type 5 (Heartbeat / Push): no addressing — Rampart issues the
///   webhook URL itself once the monitor is created.
fn endpoint_for(
    type_code: Option<i32>,
    sub_type_code: Option<i32>,
    raw_url: Option<&str>,
    raw: &Value,
) -> (Option<String>, Option<String>, Option<i32>) {
    match type_code {
        Some(1) | Some(2) => (raw_url.map(String::from), None, None),
        Some(3) => {
            // Ping target lives in `url` per UptimeRobot's API; strip any
            // accidental scheme/path so Rampart's Ping probe gets a bare
            // host.
            let host = raw_url.map(strip_scheme_and_path).map(String::from);
            (None, host, None)
        }
        Some(4) => {
            let host = raw_url.map(strip_scheme_and_path).map(String::from);
            let explicit_port = numeric_field(raw, "port");
            let port = explicit_port.or_else(|| port_for_sub_type(sub_type_code));
            (None, host, port)
        }
        Some(5) => (None, None, None),
        _ => (raw_url.map(String::from), None, None),
    }
}

/// Well-known TCP ports keyed by UptimeRobot `sub_type` codes. Used as
/// the fallback when an UptimeRobot port-monitor record omits the
/// explicit `port` field (rare but legal for HTTP / HTTPS sub-types).
fn port_for_sub_type(sub_type_code: Option<i32>) -> Option<i32> {
    match sub_type_code? {
        1 => Some(80),
        2 => Some(443),
        3 => Some(21),
        4 => Some(25),
        5 => Some(110),
        6 => Some(143),
        _ => None,
    }
}

/// Strip `scheme://` and any trailing `/path` from `raw`, returning
/// `host[:port]`-shaped or bare-host output. UptimeRobot's port-monitor
/// UI sometimes accepts a full URL even though the probe only cares
/// about the host; we normalise here so the resulting `hostname` field
/// is a clean DNS label.
fn strip_scheme_and_path(raw: &str) -> &str {
    let after_scheme = match raw.find("://") {
        Some(i) => &raw[i + 3..],
        None => raw,
    };
    match after_scheme.find('/') {
        Some(i) => &after_scheme[..i],
        None => after_scheme,
    }
}

/// Build the human-readable source-kind label printed in skip lines.
/// `type` alone for types 1 / 2 / 3 / 5; `4/sub_type` for port shapes
/// so the operator can see which protocol slot failed to map.
fn format_kind_label(type_code: Option<i32>, sub_type_code: Option<i32>) -> String {
    match (type_code, sub_type_code) {
        (Some(4), Some(s)) => format!("4/{s}"),
        (Some(t), _) => t.to_string(),
        (None, _) => "<missing>".to_string(),
    }
}

/// Pull a string out of the JSON object. Accepts both `"foo"` and `null`
/// (returns `None` for null), and tolerates a missing key.
fn string_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

/// Pull a numeric-ish field out of the JSON. UptimeRobot's documented
/// schema ships bare JSON numbers, but accept stringified numbers
/// defensively so the importer doesn't choke on either flavour of dump.
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
    fn maps_http_keyword_ping_with_correct_fields() {
        let json = r#"{"monitors":[
            {"id": 1, "friendly_name": "Plain HTTP", "type": 1,
             "url": "https://a.example.com", "interval": 60, "timeout": 10},
            {"id": 2, "friendly_name": "Keyword check", "type": 2,
             "url": "https://b.example.com", "keyword_value": "OK",
             "interval": 120, "timeout": 10},
            {"id": 3, "friendly_name": "Ping it", "type": 3,
             "url": "10.0.0.1", "interval": 60}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped.len(), 3);
        assert_eq!(plan.mapped[0].mapped_kind, MonitorKind::Http);
        assert_eq!(
            plan.mapped[0].new_monitor.url.as_deref(),
            Some("https://a.example.com")
        );
        assert_eq!(plan.mapped[1].mapped_kind, MonitorKind::Keyword);
        assert_eq!(
            plan.mapped[1].new_monitor.config["keyword"].as_str(),
            Some("OK")
        );
        assert_eq!(plan.mapped[2].mapped_kind, MonitorKind::Ping);
        assert_eq!(
            plan.mapped[2].new_monitor.hostname.as_deref(),
            Some("10.0.0.1")
        );
    }

    #[test]
    fn port_sub_types_map_to_protocol_probes() {
        let json = r#"{"monitors":[
            {"id": 1, "friendly_name": "FTP", "type": 4, "sub_type": 3,
             "url": "ftp.example.com", "port": 21},
            {"id": 2, "friendly_name": "SMTP", "type": 4, "sub_type": 4,
             "url": "smtp.example.com", "port": 25},
            {"id": 3, "friendly_name": "POP3", "type": 4, "sub_type": 5,
             "url": "pop3.example.com", "port": 110},
            {"id": 4, "friendly_name": "IMAP", "type": 4, "sub_type": 6,
             "url": "imap.example.com", "port": 143},
            {"id": 5, "friendly_name": "Custom TCP", "type": 4, "sub_type": 99,
             "url": "redis.example.com", "port": 6379}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].mapped_kind, MonitorKind::Ftp);
        assert_eq!(plan.mapped[1].mapped_kind, MonitorKind::Smtp);
        assert_eq!(plan.mapped[2].mapped_kind, MonitorKind::Pop3);
        assert_eq!(plan.mapped[3].mapped_kind, MonitorKind::Imap);
        assert_eq!(plan.mapped[4].mapped_kind, MonitorKind::Tcp);
        assert_eq!(plan.mapped[4].new_monitor.port, Some(6379));
    }

    #[test]
    fn port_http_sub_type_defaults_to_80() {
        // No explicit `port`: HTTP sub_type 1 fills in 80.
        let json = r#"{"monitors":[
            {"id": 1, "friendly_name": "HTTP port", "type": 4, "sub_type": 1,
             "url": "example.com"}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].mapped_kind, MonitorKind::Tcp);
        assert_eq!(plan.mapped[0].new_monitor.port, Some(80));
    }

    #[test]
    fn heartbeat_maps_to_push() {
        let json = r#"{"monitors":[
            {"id": 1, "friendly_name": "Cron heartbeat", "type": 5, "interval": 300}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].mapped_kind, MonitorKind::Push);
        assert!(plan.mapped[0].new_monitor.url.is_none());
    }

    #[test]
    fn skips_unknown_type_code() {
        let json = r#"{"monitors":[
            {"id": 1, "friendly_name": "Future kind", "type": 99}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert!(plan.mapped.is_empty());
        assert_eq!(plan.skipped.len(), 1);
        assert!(plan.skipped[0].reason.contains("99"));
    }

    #[test]
    fn missing_monitors_array_errors() {
        let json = r#"{"stat":"ok"}"#;
        let err = parse_and_map(json).unwrap_err();
        // Either Parse (no field) or NoMonitors — both are acceptable
        // "operator pointed us at the wrong file" signals.
        assert!(matches!(
            err,
            ImportError::Parse(_) | ImportError::NoMonitors
        ));
    }

    #[test]
    fn interval_clamped_to_validation_range() {
        let json = r#"{"monitors":[
            {"id": 1, "friendly_name": "fast", "type": 1, "url": "https://x", "interval": 5},
            {"id": 2, "friendly_name": "slow", "type": 1, "url": "https://y", "interval": 99999}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].new_monitor.interval_seconds, 10);
        assert_eq!(plan.mapped[1].new_monitor.interval_seconds, 86400);
    }

    #[test]
    fn strip_scheme_handles_bare_host_or_url() {
        assert_eq!(strip_scheme_and_path("example.com"), "example.com");
        assert_eq!(strip_scheme_and_path("https://example.com"), "example.com");
        assert_eq!(
            strip_scheme_and_path("https://example.com/path"),
            "example.com"
        );
        assert_eq!(strip_scheme_and_path("10.0.0.1"), "10.0.0.1");
    }
}
