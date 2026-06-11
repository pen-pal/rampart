//! Cachet status-page component importer.
//!
//! Reads a Cachet `GET /api/v1/components` JSON dump — operators capture
//! one with their own API token, save it to disk, hand the path to
//! `rampart-import cachet`. We don't reach out to the Cachet API
//! ourselves: importers are one-shot, offline tools by design (see
//! `docs/IMPORTERS.md` + `CONTRIBUTING.md` "Importers" bullet).
//!
//! ## Mapping
//!
//! Cachet is a status-page system, not a prober: its **components** are
//! abstract service entries carrying a `status` integer
//! (`1`=operational .. `4`=major outage) and an optional `link` URL.
//! There is no probe target unless the operator filled in `link`, so the
//! mapping is one-dimensional:
//!
//! - `link` is an `http(s)://` URL -> `Http` (the `link` becomes the
//!   monitor `url`).
//! - `link` empty / missing / not http(s) -> skipped with a warn:
//!   Cachet components can be manual-only (operators flip the status by
//!   hand) so there's nothing for Rampart to probe.
//!
//! ## Field translation
//!
//! | Cachet            | Rampart `NewMonitor`                                |
//! | ----------------- | --------------------------------------------------- |
//! | `name`            | `name` (required)                                   |
//! | `link`            | `url`                                               |
//! | (no interval)     | `interval_seconds` = `60` (Cachet exports carry no  |
//! |                   | probe cadence — components are display-only)        |

use serde::Deserialize;
use serde_json::Value;
use tracing::warn;

use rampart_core::monitor::NewMonitor;
use rampart_core::MonitorKind;

pub use super::ImportError;
use super::{ImportPlan, MappedMonitor, SkippedMonitor};

/// The minimal shape of the top-level Cachet components export. Real
/// exports carry many more fields per component (description, order,
/// group_id, enabled, …); we deserialise into `serde_json::Value` so we
/// can pick out only the ones we need and ignore the rest.
#[derive(Debug, Deserialize)]
struct Export {
    data: Vec<Value>,
}

/// Parse a Cachet components export and map every component with a
/// probeable `link` onto a `NewMonitor`. Returns the mapped list + a
/// list of skipped components (with reasons). Pure function — no I/O, no
/// DB; the integration test uses this directly without standing up
/// Postgres.
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
                    "skip: unsupported cachet component",
                );
                plan.skipped.push(s);
            }
        }
    }
    Ok(plan)
}

/// Map a single Cachet component onto a Rampart `NewMonitor`. Returns
/// the mapped form or a `SkippedMonitor` describing why we couldn't
/// translate this one. Cachet's source-kind label is constant
/// (`component`) since every entry is the same shape.
fn map_one(raw: &Value) -> Result<MappedMonitor, SkippedMonitor> {
    let source_kind = "component".to_string();
    let source_name = string_field(raw, "name").unwrap_or_else(|| "<unnamed>".to_string());

    if source_name == "<unnamed>" {
        return Err(SkippedMonitor {
            source_name,
            source_kind,
            reason: "missing name".into(),
        });
    }

    let link = string_field(raw, "link").filter(|s| !s.is_empty());

    // A Cachet component is only probeable when its `link` is an
    // http(s) URL — anything else is a manual-only status entry with
    // no target to probe.
    let url = match link {
        Some(l) if is_http_url(&l) => l,
        Some(l) => {
            return Err(SkippedMonitor {
                source_name,
                source_kind,
                reason: format!("link `{l}` is not an http(s) URL — no probe target"),
            });
        }
        None => {
            return Err(SkippedMonitor {
                source_name,
                source_kind,
                reason: "no link — manual-only component, no probe target".into(),
            });
        }
    };

    let new_monitor = NewMonitor {
        name: source_name.clone(),
        kind: MonitorKind::Http,
        url: Some(url),
        hostname: None,
        port: None,
        config: Value::Object(serde_json::Map::new()),
        interval_seconds: 60,
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
        mapped_kind: MonitorKind::Http,
        new_monitor,
    })
}

/// True when the string parses as an `http://` or `https://` URL.
fn is_http_url(s: &str) -> bool {
    let lower = s.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
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
    fn http_link_maps_to_http() {
        let json = r#"{"data":[
            {"id":1,"name":"API","link":"https://api.example.com","status":1},
            {"id":2,"name":"Web","link":"http://www.example.com","status":1}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped.len(), 2);
        for m in &plan.mapped {
            assert_eq!(m.mapped_kind, MonitorKind::Http);
        }
    }

    #[test]
    fn empty_link_is_skipped() {
        let json = r#"{"data":[
            {"id":1,"name":"Manual service","link":"","status":1}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert!(plan.mapped.is_empty());
        assert_eq!(plan.skipped.len(), 1);
    }

    #[test]
    fn missing_link_is_skipped() {
        let json = r#"{"data":[
            {"id":1,"name":"No link","status":2}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert!(plan.mapped.is_empty());
        assert_eq!(plan.skipped.len(), 1);
    }

    #[test]
    fn non_http_link_is_skipped() {
        let json = r#"{"data":[
            {"id":1,"name":"FTP service","link":"ftp://files.example.com","status":1}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert!(plan.mapped.is_empty());
        assert_eq!(plan.skipped.len(), 1);
    }

    #[test]
    fn fields_translate_correctly() {
        let json = r#"{"data":[
            {"id":7,"name":"My component","link":"https://x.example.com","status":1}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        let m = &plan.mapped[0];
        assert_eq!(m.new_monitor.name, "My component");
        assert_eq!(m.new_monitor.url.as_deref(), Some("https://x.example.com"));
        assert_eq!(m.new_monitor.interval_seconds, 60);
        assert_eq!(m.new_monitor.http_method, "GET");
    }

    #[test]
    fn missing_name_is_skipped() {
        let json = r#"{"data":[
            {"id":1,"link":"https://x","status":1}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert!(plan.mapped.is_empty());
        assert_eq!(plan.skipped.len(), 1);
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
}
