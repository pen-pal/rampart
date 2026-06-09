//! Site24x7 monitor export importer.
//!
//! Reads a Site24x7 `GET /api/monitors` JSON dump — operators capture
//! one with their own OAuth token, save it to disk, hand the path to
//! `rampart-import site24x7`. We don't reach out to the Site24x7 API
//! ourselves: importers are one-shot, offline tools by design (see
//! `docs/IMPORTERS.md` + `CONTRIBUTING.md` "Importers" bullet).
//!
//! ## Mapping
//!
//! See the table in `docs/IMPORTERS.md`. Anything not in the table is
//! skipped with a `tracing::warn!` line so the operator can hand-port
//! the unusual monitors (real-browser checks, AWS/EC2 introspection,
//! Server / VMware agent monitors, etc.) after the import.
//!
//! ## Field translation
//!
//! | Site24x7              | Rampart `NewMonitor`         |
//! | --------------------- | ---------------------------- |
//! | `display_name`        | `name`                       |
//! | `website` / `url`     | `url`                        |
//! | `host_name`           | `hostname`                   |
//! | `port`                | `port` (i32)                 |
//! | `check_frequency` (s) | `interval_seconds`           |
//! | `timeout` (s)         | `timeout_seconds`            |
//! | `retries` /           | `max_retries`                |
//! | `threshold_count`     |                              |
//! | `http_method`         | `http_method` (mapped below) |
//!
//! Site24x7's `check_frequency` is documented as "in seconds" but every
//! sample dump I've seen ships it as a JSON *string* (`"60"`). We accept
//! both shapes; missing / unparseable falls back to the API defaults
//! (60s interval, 16s timeout — matches `NewMonitor`'s serde defaults).

use serde::Deserialize;
use serde_json::Value;
use tracing::warn;

use rampart_core::monitor::NewMonitor;
use rampart_core::MonitorKind;

pub use super::ImportError;
use super::{ImportPlan, MappedMonitor, SkippedMonitor};

/// The minimal shape of the top-level Site24x7 export. Real exports
/// carry dozens more fields per monitor; we deserialise into
/// `serde_json::Value` so we can pick out only the ones we need and
/// ignore the rest without listing every possible variant.
#[derive(Debug, Deserialize)]
struct Export {
    monitors: Vec<Value>,
}

/// Parse a Site24x7 export and map every recognisable entry onto a
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
                    "skip: unsupported site24x7 monitor",
                );
                plan.skipped.push(s);
            }
        }
    }
    Ok(plan)
}

/// Map a single Site24x7 monitor object onto a Rampart `NewMonitor`.
/// Returns the mapped form or a `SkippedMonitor` describing why we
/// couldn't translate this one.
fn map_one(raw: &Value) -> Result<MappedMonitor, SkippedMonitor> {
    let source_kind = string_field(raw, "type").unwrap_or_else(|| "<missing>".to_string());
    let source_name = string_field(raw, "display_name").unwrap_or_else(|| "<unnamed>".to_string());

    // matching_keyword: presence flips URL/REST_API into the Keyword
    // probe so the imported monitor still asserts on the body. See
    // `docs/IMPORTERS.md` for the rationale.
    let has_keyword = raw
        .get("matching_keyword")
        .map(|v| !v.is_null())
        .unwrap_or(false);
    // response_content_check is the REST_API equivalent — when set,
    // the monitor asserts on a JSON body fragment. Closest Rampart
    // probe is JsonQuery.
    let has_json_check = raw
        .get("response_content_check")
        .map(|v| !v.is_null())
        .unwrap_or(false);

    let mapped_kind = match map_kind(&source_kind, has_keyword, has_json_check) {
        Some(k) => k,
        None => {
            let reason = format!("unsupported site24x7 type `{source_kind}`");
            return Err(SkippedMonitor {
                source_name,
                source_kind,
                reason,
            });
        }
    };

    let name = if source_name == "<unnamed>" {
        return Err(SkippedMonitor {
            source_name,
            source_kind,
            reason: "missing display_name".into(),
        });
    } else {
        source_name.clone()
    };

    // Pull out the endpoint addressing fields. `website` is the Site24x7
    // name for URL-shape probes; `host_name` is the network-primitive
    // shape (Ping, DNS, database, etc).
    let url = string_field(raw, "website").or_else(|| string_field(raw, "url"));
    let hostname = string_field(raw, "host_name").or_else(|| string_field(raw, "hostname"));
    let port = numeric_field(raw, "port");

    let interval_seconds = numeric_field(raw, "check_frequency")
        .unwrap_or(60)
        .clamp(10, 86400);
    let timeout_seconds = numeric_field(raw, "timeout").unwrap_or(16).clamp(1, 600);
    let max_retries = numeric_field(raw, "retries")
        .or_else(|| numeric_field(raw, "threshold_count"))
        .unwrap_or(0)
        .max(0);

    let http_method = string_field(raw, "http_method")
        .map(translate_http_method)
        .unwrap_or_else(|| "GET".into());

    let new_monitor = NewMonitor {
        name,
        kind: mapped_kind,
        url,
        hostname,
        port,
        config: serde_json::Value::Object(Default::default()),
        interval_seconds,
        timeout_seconds,
        max_retries,
        retry_interval_sec: 60,
        resend_interval_sec: 0,
        upside_down: false,
        http_method,
        http_body: None,
        http_headers: None,
        accepted_statuses: vec![200, 201, 202, 203, 204, 205, 206, 207, 208, 226],
        follow_redirect: true,
        ignore_tls: false,
        proxy_id: None,
        group_id: None,
    };

    Ok(MappedMonitor {
        source_name,
        source_kind,
        mapped_kind,
        new_monitor,
    })
}

/// Site24x7 type constant -> Rampart probe kind. Returns `None` for
/// types Rampart has no equivalent for (real-browser, AWS introspection,
/// VMware/Hyper-V agent monitors, etc.); those get reported as skipped.
fn map_kind(source: &str, has_keyword: bool, has_json_check: bool) -> Option<MonitorKind> {
    // Normalise: Site24x7 docs are inconsistent — some types appear as
    // `SSL_CERT` in one place and `SSL_CERTIFICATE` in another; the
    // export JSON has historically shipped uppercase with underscores.
    let s = source.to_ascii_uppercase();
    let kind = match s.as_str() {
        // HTTP family
        "URL" | "HOMEPAGE" => {
            if has_keyword {
                MonitorKind::Keyword
            } else {
                MonitorKind::Http
            }
        }
        // REST API: JsonQuery when there's a body assertion, else plain HTTP.
        "RESTAPI" | "REST_API" => {
            if has_json_check {
                MonitorKind::JsonQuery
            } else if has_keyword {
                MonitorKind::Keyword
            } else {
                MonitorKind::Http
            }
        }
        "SOAP" => MonitorKind::Http,
        "HEARTBEAT" | "CRON" => MonitorKind::Push,
        "PING" => MonitorKind::Ping,
        "DNS" => MonitorKind::Dns,
        "SSL_CERT" | "SSL_CERTIFICATE" | "SSLCERT" => MonitorKind::Tls,
        "PORT" | "TCP" => MonitorKind::Tcp,
        "POSTGRES" | "POSTGRESQL" => MonitorKind::Postgres,
        "MYSQL" => MonitorKind::Mysql,
        "MSSQL" | "SQLSERVER" => MonitorKind::Mssql,
        "MONGODB" | "MONGO" => MonitorKind::Mongodb,
        "FTP" | "FTPS" | "SFTP" | "PORT_FTP" => MonitorKind::Ftp,
        "SSH" => MonitorKind::Ssh,
        "SMTP" | "SMTPS" | "PORT-SMTP" | "PORT_SMTP" => MonitorKind::Smtp,
        "IMAP" | "IMAPS" => MonitorKind::Imap,
        "POP" | "POP3" | "POPS" | "PORT-POP" | "PORT_POP" => MonitorKind::Pop3,
        "WEBSOCKET" | "WEBSOCKETS" => MonitorKind::Websocket,
        "DOMAINEXPIRY" | "DOMAIN_EXPIRY" | "DOMAIN" => MonitorKind::Domain,
        "MQTT" => MonitorKind::Mqtt,
        "GRPC" => MonitorKind::Grpc,
        _ => return None,
    };
    Some(kind)
}

/// Site24x7 ships `http_method` as a single-letter code (`G` / `P` /
/// `U` / `D` / `H`). Rampart stores the full verb. Anything else
/// passes through verbatim (already a full word like `POST`).
fn translate_http_method(raw: String) -> String {
    match raw.trim().to_ascii_uppercase().as_str() {
        "G" => "GET".into(),
        "P" => "POST".into(),
        "U" => "PUT".into(),
        "D" => "DELETE".into(),
        "H" => "HEAD".into(),
        // Already a full verb (POST/PUT/PATCH/etc) or some future code we
        // don't recognise — pass through; the route layer will reject
        // genuinely-invalid values.
        other => other.to_string(),
    }
}

/// Pull a string out of the JSON object. Accepts both `"foo"` and `null`
/// (returns `None` for null), and tolerates a missing key.
fn string_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

/// Pull a numeric-ish field out of the JSON. Site24x7's API ships
/// numbers-as-strings (`"60"`) in some places and bare numbers in
/// others; we accept both so the importer doesn't choke on either
/// flavour of dump.
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
    fn maps_url_and_url_with_keyword() {
        let json = r#"{"monitors":[
            {"display_name": "Plain URL", "type": "URL", "website": "https://a.example.com"},
            {"display_name": "Keyword URL", "type": "URL", "website": "https://b.example.com",
             "matching_keyword": {"value": "OK", "severity": 2}}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped.len(), 2);
        assert_eq!(plan.mapped[0].mapped_kind, MonitorKind::Http);
        assert_eq!(plan.mapped[1].mapped_kind, MonitorKind::Keyword);
        assert!(plan.skipped.is_empty());
    }

    #[test]
    fn maps_postgres_and_ping_with_default_interval() {
        let json = r#"{"monitors":[
            {"display_name": "DB", "type": "POSTGRES", "host_name": "db", "port": 5432},
            {"display_name": "Box", "type": "PING", "host_name": "10.0.0.1"}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped.len(), 2);
        assert_eq!(plan.mapped[0].mapped_kind, MonitorKind::Postgres);
        assert_eq!(plan.mapped[0].new_monitor.port, Some(5432));
        assert_eq!(plan.mapped[1].mapped_kind, MonitorKind::Ping);
        // No check_frequency in the fixture -> default 60s applies.
        assert_eq!(plan.mapped[1].new_monitor.interval_seconds, 60);
    }

    #[test]
    fn skips_unsupported_kind() {
        let json = r#"{"monitors":[
            {"display_name": "Browser", "type": "REALBROWSER", "website": "https://x"},
            {"display_name": "EC2", "type": "EC2INSTANCE"}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert!(plan.mapped.is_empty());
        assert_eq!(plan.skipped.len(), 2);
        assert!(plan.skipped[0].reason.contains("REALBROWSER"));
    }

    #[test]
    fn http_method_letters_translate() {
        assert_eq!(translate_http_method("G".into()), "GET");
        assert_eq!(translate_http_method("P".into()), "POST");
        assert_eq!(translate_http_method("POST".into()), "POST");
    }

    #[test]
    fn check_frequency_accepts_string_or_number() {
        let json = r#"{"monitors":[
            {"display_name": "a", "type": "URL", "website": "https://a", "check_frequency": "120"},
            {"display_name": "b", "type": "URL", "website": "https://b", "check_frequency": 300}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].new_monitor.interval_seconds, 120);
        assert_eq!(plan.mapped[1].new_monitor.interval_seconds, 300);
    }

    #[test]
    fn rest_api_with_response_content_check_becomes_json_query() {
        let json = r#"{"monitors":[
            {"display_name": "api", "type": "RESTAPI", "website": "https://api/health",
             "response_content_check": {"jsonpath": "$.status", "expected": "ok"}}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].mapped_kind, MonitorKind::JsonQuery);
    }

    #[test]
    fn missing_monitors_array_errors() {
        let json = r#"{"data": []}"#;
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
        // NewMonitor validation enforces 10 <= interval_seconds <= 86400.
        // The importer clamps to that range so the subsequent insert
        // doesn't trip a validation error.
        let json = r#"{"monitors":[
            {"display_name": "fast", "type": "URL", "website": "https://x", "check_frequency": 5},
            {"display_name": "slow", "type": "URL", "website": "https://y", "check_frequency": 99999}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].new_monitor.interval_seconds, 10);
        assert_eq!(plan.mapped[1].new_monitor.interval_seconds, 86400);
    }
}
