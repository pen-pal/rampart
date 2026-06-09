//! Checkly check importer.
//!
//! Reads a Checkly `GET /v1/checks` JSON dump — operators capture one
//! with their own API token, save it to disk, hand the path to
//! `rampart-import checkly`. We don't reach out to the Checkly API
//! ourselves: importers are one-shot, offline tools by design (see
//! `docs/IMPORTERS.md` + `CONTRIBUTING.md` "Importers" bullet).
//!
//! > Checkly's listing endpoint returns a bare JSON array at the top
//! > level (not a `{"checks":[…]}` envelope), so we deserialise straight
//! > into `Vec<Value>`.
//!
//! ## Mapping
//!
//! Each check carries a `checkType` constant that selects the probe
//! family:
//!
//! | Checkly `checkType` | Rampart `MonitorKind` |
//! | ------------------- | --------------------- |
//! | `API`               | `Http`                |
//! | `BROWSER`           | `Browser`             |
//! | `TCP`               | `Tcp`                 |
//!
//! Anything else is skipped with a `tracing::warn!` line so the operator
//! can hand-port it.
//!
//! ## Field translation
//!
//! | Checkly                 | Rampart `NewMonitor`                                        |
//! | ----------------------- | ----------------------------------------------------------- |
//! | `name`                  | `name` (required)                                           |
//! | `request.url`           | `url` (`API`)                                               |
//! | `request.method`        | `http_method` (`API`; defaults to `GET`)                    |
//! | `tcp.host`              | `hostname` (`TCP`)                                          |
//! | `tcp.port`              | `port` (`TCP`)                                              |
//! | `frequency` (minutes)   | `interval_seconds` = `frequency * 60`, floored to `60`      |

use serde_json::Value;
use tracing::warn;

use rampart_core::monitor::NewMonitor;
use rampart_core::MonitorKind;

pub use super::ImportError;
use super::{ImportPlan, MappedMonitor, SkippedMonitor};

/// Parse a Checkly checks export and map every recognisable entry onto a
/// `NewMonitor`. Returns the mapped list + a list of skipped checks
/// (with reasons). Pure function — no I/O, no DB; the integration test
/// uses this directly without standing up Postgres.
///
/// The Checkly listing endpoint returns a bare JSON array at the top
/// level (not the `{"data":[…]}` envelope most other importers use), so
/// we deserialise straight into `Vec<Value>`.
pub fn parse_and_map(json: &str) -> Result<ImportPlan, ImportError> {
    let checks: Vec<Value> = serde_json::from_str(json)?;
    if checks.is_empty() {
        return Err(ImportError::NoMonitors);
    }

    let mut plan = ImportPlan::default();
    for raw in checks {
        match map_one(&raw) {
            Ok(m) => plan.mapped.push(m),
            Err(s) => {
                warn!(
                    source_name = %s.source_name,
                    source_kind = %s.source_kind,
                    reason = %s.reason,
                    "skip: unsupported checkly check",
                );
                plan.skipped.push(s);
            }
        }
    }
    Ok(plan)
}

/// Map a single Checkly check onto a Rampart `NewMonitor`. Returns the
/// mapped form or a `SkippedMonitor` describing why we couldn't
/// translate this one.
fn map_one(raw: &Value) -> Result<MappedMonitor, SkippedMonitor> {
    let check_type = string_field(raw, "checkType").unwrap_or_else(|| "<missing>".to_string());
    let source_kind = check_type.clone();
    let source_name = string_field(raw, "name").unwrap_or_else(|| "<unnamed>".to_string());

    let mapped_kind = match map_kind(&check_type) {
        Some(k) => k,
        None => {
            return Err(SkippedMonitor {
                source_name,
                source_kind,
                reason: format!("unsupported checkly checkType `{check_type}`"),
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

    // Pull kind-specific endpoint fields out of the nested objects.
    let mut url = None;
    let mut hostname = None;
    let mut port = None;
    let mut http_method = "GET".to_string();

    match mapped_kind {
        MonitorKind::Http => {
            // `API` checks carry the target under `request`.
            let request = raw.get("request");
            url = request
                .and_then(|r| r.get("url"))
                .and_then(|u| u.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());
            if let Some(m) = request
                .and_then(|r| r.get("method"))
                .and_then(|m| m.as_str())
                .map(|s| s.trim().to_ascii_uppercase())
                .filter(|s| !s.is_empty())
            {
                http_method = m;
            }
        }
        MonitorKind::Tcp => {
            // `TCP` checks carry the target under `tcp`.
            let tcp = raw.get("tcp");
            hostname = tcp
                .and_then(|t| t.get("host"))
                .and_then(|h| h.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());
            port = tcp
                .and_then(|t| t.get("port"))
                .and_then(numeric_value)
                .filter(|p| *p > 0);
        }
        // `BROWSER` checks have no host/url target on the listing shape;
        // the operator wires up `config.renderer_url` afterwards.
        _ => {}
    }

    // Checkly ships the cadence as whole minutes; Rampart stores seconds.
    // Floor at 60s so a sub-minute interval never produces a value below
    // Rampart's minimum.
    let frequency_minutes = numeric_field(raw, "frequency").unwrap_or(1);
    let interval_seconds = (frequency_minutes * 60).max(60);

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
        http_method,
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

/// Checkly `checkType` -> Rampart probe kind. Returns `None` for shapes
/// Rampart has no equivalent for; those get reported as skipped. Matched
/// case-insensitively for defensiveness against API casing drift.
fn map_kind(check_type: &str) -> Option<MonitorKind> {
    let kind = match check_type.trim().to_ascii_uppercase().as_str() {
        "API" => MonitorKind::Http,
        "BROWSER" => MonitorKind::Browser,
        "TCP" => MonitorKind::Tcp,
        _ => return None,
    };
    Some(kind)
}

/// Pull a string out of the JSON object. Accepts both `"foo"` and `null`
/// (returns `None` for null), and tolerates a missing key.
fn string_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

/// Pull a numeric-ish field out of the JSON. Checkly ships numbers as
/// bare JSON numbers in the documented schema, but accept stringified
/// numbers defensively.
fn numeric_field(v: &Value, key: &str) -> Option<i32> {
    v.get(key).and_then(numeric_value)
}

/// Coerce a single JSON value into an `i32`, tolerating stringified
/// numbers.
fn numeric_value(v: &Value) -> Option<i32> {
    match v {
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
        let json = r#"[
            {"id":"a","name":"web","checkType":"API","request":{"url":"https://example.com","method":"GET"},"frequency":1},
            {"id":"b","name":"journey","checkType":"BROWSER","frequency":5},
            {"id":"c","name":"db","checkType":"TCP","tcp":{"host":"db.example.com","port":5432},"frequency":1}
        ]"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped.len(), 3);
        assert_eq!(plan.mapped[0].mapped_kind, MonitorKind::Http);
        assert_eq!(plan.mapped[1].mapped_kind, MonitorKind::Browser);
        assert_eq!(plan.mapped[2].mapped_kind, MonitorKind::Tcp);
    }

    #[test]
    fn api_uses_request_url_and_method() {
        let json = r#"[
            {"id":"a","name":"web","checkType":"API","request":{"url":"https://example.com/health","method":"post"},"frequency":1}
        ]"#;
        let plan = parse_and_map(json).unwrap();
        let m = &plan.mapped[0];
        assert_eq!(
            m.new_monitor.url.as_deref(),
            Some("https://example.com/health")
        );
        assert!(m.new_monitor.hostname.is_none());
        assert_eq!(m.new_monitor.http_method, "POST");
    }

    #[test]
    fn tcp_uses_nested_host_and_port() {
        let json = r#"[
            {"id":"c","name":"db","checkType":"TCP","tcp":{"host":"db.example.com","port":5432},"frequency":1}
        ]"#;
        let plan = parse_and_map(json).unwrap();
        let m = &plan.mapped[0];
        assert!(m.new_monitor.url.is_none());
        assert_eq!(m.new_monitor.hostname.as_deref(), Some("db.example.com"));
        assert_eq!(m.new_monitor.port, Some(5432));
    }

    #[test]
    fn frequency_minutes_convert_to_seconds_floored() {
        let json = r#"[
            {"id":"a","name":"a","checkType":"API","request":{"url":"https://a"},"frequency":5},
            {"id":"b","name":"b","checkType":"API","request":{"url":"https://b"},"frequency":0}
        ]"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].new_monitor.interval_seconds, 300);
        assert_eq!(plan.mapped[1].new_monitor.interval_seconds, 60);
    }

    #[test]
    fn unknown_check_type_is_skipped() {
        let json = r#"[
            {"id":"x","name":"thing","checkType":"HEARTBEAT","frequency":1}
        ]"#;
        let plan = parse_and_map(json).unwrap();
        assert!(plan.mapped.is_empty());
        assert_eq!(plan.skipped.len(), 1);
        assert_eq!(plan.skipped[0].source_kind, "HEARTBEAT");
    }

    #[test]
    fn empty_array_errors() {
        let json = r#"[]"#;
        let err = parse_and_map(json).unwrap_err();
        assert!(matches!(
            err,
            ImportError::Parse(_) | ImportError::NoMonitors
        ));
    }
}
