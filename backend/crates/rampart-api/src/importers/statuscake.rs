//! StatusCake monitor export importer.
//!
//! Reads a StatusCake `GET /v1/uptime` JSON dump — operators capture
//! one with their own API token, save it to disk, hand the path to
//! `rampart-import statuscake`. We don't reach out to the StatusCake
//! API ourselves: importers are one-shot, offline tools by design (see
//! `docs/IMPORTERS.md` + `CONTRIBUTING.md` "Importers" bullet).
//!
//! ## Mapping
//!
//! StatusCake ships the response as `{"data":[{"id":"…","name":"…",
//! "test_type":"…", …}, …]}`. The `test_type` field selects the probe
//! family. See the table in `docs/IMPORTERS.md`. Anything not in the
//! table is skipped with a `tracing::warn!` line so the operator can
//! hand-port the unusual monitors after the import.
//!
//! ## Field translation
//!
//! | StatusCake                     | Rampart `NewMonitor`              |
//! | ------------------------------ | --------------------------------- |
//! | `name`                         | `name` (required)                 |
//! | `website_url`                  | `url`                             |
//! | `test_type`                    | (selects probe kind)              |
//! | `check_rate` (seconds)         | `interval_seconds` (clamped to `10..=86400`; default `60`) |
//! | `timeout` (seconds)            | `timeout_seconds` (clamped to `1..=600`; default `16`) |
//!
//! StatusCake ships `check_rate` / `timeout` as bare JSON numbers per
//! the documented schema, but we accept stringified numbers defensively.

use serde::Deserialize;
use serde_json::Value;
use tracing::warn;

use rampart_core::monitor::NewMonitor;
use rampart_core::MonitorKind;

pub use super::ImportError;
use super::{ImportPlan, MappedMonitor, SkippedMonitor};

/// The minimal shape of the top-level StatusCake export. Real exports
/// carry many more fields per monitor; we deserialise into
/// `serde_json::Value` so we can pick out only the ones we need and
/// ignore the rest without listing every possible variant.
#[derive(Debug, Deserialize)]
struct Export {
    data: Vec<Value>,
}

/// Parse a StatusCake export and map every recognisable entry onto a
/// `NewMonitor`. Returns the mapped list + a list of skipped monitors
/// (with reasons). Pure function — no I/O, no DB; the integration test
/// uses this directly without standing up Postgres.
pub fn parse_and_map(json: &str) -> Result<ImportPlan, ImportError> {
    let export: Export = serde_json::from_str(json)?;
    if export.data.is_empty() {
        return Err(ImportError::NoMonitors);
    }

    let mut plan = ImportPlan::default();
    for raw in export.data {
        match map_one(&raw) {
            Ok(m) => plan.mapped.push(m),
            Err(s) => {
                warn!(
                    source_name = %s.source_name,
                    source_kind = %s.source_kind,
                    reason = %s.reason,
                    "skip: unsupported statuscake monitor",
                );
                plan.skipped.push(s);
            }
        }
    }
    Ok(plan)
}

/// Map a single StatusCake monitor onto a Rampart `NewMonitor`.
/// Returns the mapped form or a `SkippedMonitor` describing why we
/// couldn't translate this one.
fn map_one(raw: &Value) -> Result<MappedMonitor, SkippedMonitor> {
    let test_type = string_field(raw, "test_type").unwrap_or_else(|| "<missing>".to_string());
    let source_kind = test_type.clone();
    let source_name = string_field(raw, "name").unwrap_or_else(|| "<unnamed>".to_string());

    let mapped_kind = match map_kind(&test_type) {
        Some(k) => k,
        None => {
            let reason = format!("unsupported statuscake test_type `{source_kind}`");
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

    let url = string_field(raw, "website_url");
    let interval_seconds = numeric_field(raw, "check_rate")
        .unwrap_or(60)
        .clamp(10, 86400);
    let timeout_seconds = numeric_field(raw, "timeout").unwrap_or(16).clamp(1, 600);

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
    };

    Ok(MappedMonitor {
        source_name,
        source_kind,
        mapped_kind,
        new_monitor,
    })
}

/// StatusCake `test_type` -> Rampart probe kind. Returns `None` for
/// shapes Rampart has no equivalent for (unknown future kinds); those
/// get reported as skipped.
fn map_kind(test_type: &str) -> Option<MonitorKind> {
    let s = test_type.to_ascii_uppercase();
    let kind = match s.as_str() {
        "HTTP" => MonitorKind::Http,
        "PING" => MonitorKind::Ping,
        "TCP" => MonitorKind::Tcp,
        "DNS" => MonitorKind::Dns,
        "SMTP" => MonitorKind::Smtp,
        "SSH" => MonitorKind::Ssh,
        _ => return None,
    };
    Some(kind)
}

/// Pull a string out of the JSON object. Accepts both `"foo"` and `null`
/// (returns `None` for null), and tolerates a missing key.
fn string_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

/// Pull a numeric-ish field out of the JSON. StatusCake ships numbers
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
    fn maps_each_test_type_to_correct_kind() {
        let json = r#"{"data":[
            {"id":"1","name":"HTTP1","test_type":"HTTP","website_url":"https://a.example.com","check_rate":60,"timeout":15},
            {"id":"2","name":"PING1","test_type":"PING","website_url":"10.0.0.1","check_rate":300,"timeout":10},
            {"id":"3","name":"TCP1","test_type":"TCP","website_url":"db.example.com","check_rate":60,"timeout":15},
            {"id":"4","name":"DNS1","test_type":"DNS","website_url":"example.com","check_rate":300,"timeout":10},
            {"id":"5","name":"SMTP1","test_type":"SMTP","website_url":"smtp.example.com","check_rate":300,"timeout":20},
            {"id":"6","name":"SSH1","test_type":"SSH","website_url":"ssh.example.com","check_rate":300,"timeout":10}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped.len(), 6);
        assert_eq!(plan.mapped[0].mapped_kind, MonitorKind::Http);
        assert_eq!(plan.mapped[1].mapped_kind, MonitorKind::Ping);
        assert_eq!(plan.mapped[2].mapped_kind, MonitorKind::Tcp);
        assert_eq!(plan.mapped[3].mapped_kind, MonitorKind::Dns);
        assert_eq!(plan.mapped[4].mapped_kind, MonitorKind::Smtp);
        assert_eq!(plan.mapped[5].mapped_kind, MonitorKind::Ssh);
    }

    #[test]
    fn fields_translate_correctly() {
        let json = r#"{"data":[
            {"id":"1","name":"My HTTP","test_type":"HTTP","website_url":"https://x","check_rate":90,"timeout":12}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        let m = &plan.mapped[0];
        assert_eq!(m.new_monitor.name, "My HTTP");
        assert_eq!(m.new_monitor.url.as_deref(), Some("https://x"));
        assert_eq!(m.new_monitor.interval_seconds, 90);
        assert_eq!(m.new_monitor.timeout_seconds, 12);
    }

    #[test]
    fn skips_unknown_test_type() {
        let json = r#"{"data":[
            {"id":"1","name":"Mystery","test_type":"PUSH","website_url":"x","check_rate":60}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert!(plan.mapped.is_empty());
        assert_eq!(plan.skipped.len(), 1);
        assert_eq!(plan.skipped[0].source_kind, "PUSH");
    }

    #[test]
    fn skips_missing_name() {
        let json = r#"{"data":[
            {"id":"1","test_type":"HTTP","website_url":"https://x","check_rate":60}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert!(plan.mapped.is_empty());
        assert_eq!(plan.skipped.len(), 1);
    }

    #[test]
    fn interval_clamped_to_validation_range() {
        let json = r#"{"data":[
            {"id":"1","name":"fast","test_type":"HTTP","website_url":"https://x","check_rate":5},
            {"id":"2","name":"slow","test_type":"HTTP","website_url":"https://y","check_rate":99999}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].new_monitor.interval_seconds, 10);
        assert_eq!(plan.mapped[1].new_monitor.interval_seconds, 86400);
    }

    #[test]
    fn missing_data_array_errors() {
        let json = r#"{"not_data":[]}"#;
        let err = parse_and_map(json).unwrap_err();
        assert!(matches!(
            err,
            ImportError::Parse(_) | ImportError::NoMonitors
        ));
    }

    #[test]
    fn lowercase_test_type_still_maps() {
        // StatusCake docs uppercase test_type, but tolerate lowercase
        // defensively.
        let json = r#"{"data":[
            {"id":"1","name":"a","test_type":"http","website_url":"https://x","check_rate":60}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].mapped_kind, MonitorKind::Http);
    }
}
