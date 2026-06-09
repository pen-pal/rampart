//! StatusGator service importer.
//!
//! StatusGator is itself a status-page *aggregator* — it watches the
//! public status pages of other SaaS providers and rolls their state up
//! into one dashboard. Its export is therefore a flat list of *services*
//! (the upstream products it watches), each carrying the URL of the
//! status page or product home page. Operators capture an export with
//! their own account, save it to disk, and hand the path to
//! `rampart-import statusgator`. We don't reach out to the StatusGator
//! API ourselves: importers are one-shot, offline tools by design (see
//! `docs/IMPORTERS.md` + `CONTRIBUTING.md` "Importers" bullet).
//!
//! ## Mapping
//!
//! Every service maps to a single `Http` monitor — StatusGator only
//! tracks web-reachable products, so there is no probe-family fan-out
//! the way a generic uptime tool's export has.
//!
//! | StatusGator service     | Rampart `MonitorKind` |
//! | ----------------------- | --------------------- |
//! | (any)                   | `Http`                |
//!
//! A service with neither a usable `url` nor `home_page_url` is skipped
//! with a `tracing::warn!` line so the operator can hand-port it.
//!
//! ## Field translation
//!
//! | StatusGator      | Rampart `NewMonitor`                                            |
//! | ---------------- | --------------------------------------------------------------- |
//! | `name`           | `name` (required)                                               |
//! | `url`            | `url` (preferred)                                               |
//! | `home_page_url`  | `url` (fallback when `url` is absent/empty)                     |
//! | (none)           | `interval_seconds` defaults to `300` (StatusGator carries none) |

use serde::Deserialize;
use serde_json::Value;
use tracing::warn;

use rampart_core::monitor::NewMonitor;
use rampart_core::MonitorKind;

pub use super::ImportError;
use super::{ImportPlan, MappedMonitor, SkippedMonitor};

/// StatusGator carries no per-service probe cadence (it polls upstream
/// status pages on its own schedule), so we seed a sane 5-minute
/// default.
const DEFAULT_INTERVAL_SECONDS: i32 = 300;

/// The minimal shape of the top-level StatusGator services export. Real
/// exports carry many more fields per service; we deserialise into
/// `serde_json::Value` so we can pick out only the ones we need and
/// ignore the rest.
#[derive(Debug, Deserialize)]
struct Export {
    services: Vec<Value>,
}

/// Parse a StatusGator services export and map every recognisable entry
/// onto a `NewMonitor`. Returns the mapped list + a list of skipped
/// services (with reasons). Pure function — no I/O, no DB; the
/// integration test uses this directly without standing up Postgres.
pub fn parse_and_map(json: &str) -> Result<ImportPlan, ImportError> {
    let export: Export = serde_json::from_str(json)?;
    if export.services.is_empty() {
        return Err(ImportError::NoMonitors);
    }

    let mut plan = ImportPlan::default();
    for raw in export.services {
        match map_one(&raw) {
            Ok(m) => plan.mapped.push(m),
            Err(s) => {
                warn!(
                    source_name = %s.source_name,
                    source_kind = %s.source_kind,
                    reason = %s.reason,
                    "skip: unsupported statusgator service",
                );
                plan.skipped.push(s);
            }
        }
    }
    Ok(plan)
}

/// Map a single StatusGator service onto a Rampart `NewMonitor`. Returns
/// the mapped form or a `SkippedMonitor` describing why we couldn't
/// translate this one.
fn map_one(raw: &Value) -> Result<MappedMonitor, SkippedMonitor> {
    // StatusGator only watches web products; the source "kind" is fixed.
    let source_kind = "service".to_string();
    let source_name = string_field(raw, "name").unwrap_or_else(|| "<unnamed>".to_string());

    if source_name == "<unnamed>" {
        return Err(SkippedMonitor {
            source_name,
            source_kind,
            reason: "missing name".into(),
        });
    }

    // Prefer the status-page `url`; fall back to the product home page.
    let url = string_field(raw, "url")
        .filter(|s| !s.is_empty())
        .or_else(|| string_field(raw, "home_page_url").filter(|s| !s.is_empty()));

    let url = match url {
        Some(u) => u,
        None => {
            return Err(SkippedMonitor {
                source_name,
                source_kind,
                reason: "no usable url or home_page_url".into(),
            });
        }
    };

    let mapped_kind = MonitorKind::Http;
    let new_monitor = NewMonitor {
        name: source_name.clone(),
        kind: mapped_kind,
        url: Some(url),
        hostname: None,
        port: None,
        config: Value::Object(serde_json::Map::new()),
        interval_seconds: DEFAULT_INTERVAL_SECONDS,
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

/// Pull a string out of the JSON object. Accepts both `"foo"` and `null`
/// (returns `None` for null), and tolerates a missing key.
fn string_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_every_service_to_http() {
        let json = r#"{"services":[
            {"name":"GitHub","url":"https://www.githubstatus.com","status":"up","home_page_url":"https://github.com"},
            {"name":"Stripe","url":"https://status.stripe.com","status":"warn","home_page_url":"https://stripe.com"}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped.len(), 2);
        assert!(plan
            .mapped
            .iter()
            .all(|m| m.mapped_kind == MonitorKind::Http));
    }

    #[test]
    fn falls_back_to_home_page_url() {
        let json = r#"{"services":[
            {"name":"Acme","status":"up","home_page_url":"https://acme.example.com"}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        let m = &plan.mapped[0];
        assert_eq!(
            m.new_monitor.url.as_deref(),
            Some("https://acme.example.com")
        );
        assert_eq!(m.new_monitor.interval_seconds, 300);
    }

    #[test]
    fn service_without_any_url_is_skipped() {
        let json = r#"{"services":[
            {"name":"Manual thing","status":"up"}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert!(plan.mapped.is_empty());
        assert_eq!(plan.skipped.len(), 1);
    }

    #[test]
    fn missing_services_array_errors() {
        let json = r#"{"not_services":[]}"#;
        let err = parse_and_map(json).unwrap_err();
        assert!(matches!(
            err,
            ImportError::Parse(_) | ImportError::NoMonitors
        ));
    }
}
