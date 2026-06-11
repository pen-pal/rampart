//! Datadog Synthetics monitor export importer.
//!
//! Reads a Datadog `GET /api/v1/synthetics/tests` JSON dump — operators
//! capture one with their own API key + APP key, save it to disk, hand
//! the path to `rampart-import datadog`. We don't reach out to the
//! Datadog API ourselves: importers are one-shot, offline tools by
//! design (see `docs/IMPORTERS.md` + `CONTRIBUTING.md` "Importers"
//! bullet).
//!
//! ## Mapping
//!
//! See the table in `docs/IMPORTERS.md`. The Datadog Synthetics surface
//! is `(type, subtype)` shaped — `type` is `"api"` or `"browser"`, and
//! `subtype` narrows API tests into `http` / `tcp` / `dns` / `ssl` /
//! `icmp` / `grpc` / `websocket` / `udp` / `multi`. Anything not in the
//! table (most notably `(api, multi)` — multi-step synthetics — which
//! Rampart does not model) is skipped with a `tracing::warn!` line so
//! the operator can hand-port the unusual monitors after the import.
//!
//! ## Field translation
//!
//! | Datadog                                  | Rampart `NewMonitor`         |
//! | ---------------------------------------- | ---------------------------- |
//! | `name`                                   | `name`                       |
//! | `config.request.url`                     | `url`                        |
//! | `config.request.host`                    | `hostname`                   |
//! | `config.request.port`                    | `port` (i32)                 |
//! | `config.request.method`                  | `http_method` (default GET)  |
//! | `config.request.timeout` (s)             | `timeout_seconds` (default 16) |
//! | `options.tick_every` (s)                 | `interval_seconds` (default 60) |
//! | `options.retry.count`                    | `max_retries` (default 0)    |
//!
//! Datadog's `tick_every` is documented as "in seconds" and ships as a
//! bare JSON number; same for `timeout`. We accept either a number or
//! a string-as-number defensively (Datadog has historically stringified
//! some numeric fields in older API revisions). Missing / unparseable
//! falls back to the API defaults (60s interval, 16s timeout — matches
//! `NewMonitor`'s serde defaults).

use serde::Deserialize;
use serde_json::Value;
use tracing::warn;

use rampart_core::monitor::NewMonitor;
use rampart_core::MonitorKind;

pub use super::ImportError;
use super::{ImportPlan, MappedMonitor, SkippedMonitor};

/// The minimal shape of the top-level Datadog Synthetics export. Real
/// exports carry dozens more fields per test; we deserialise into
/// `serde_json::Value` so we can pick out only the ones we need and
/// ignore the rest without listing every possible variant.
#[derive(Debug, Deserialize)]
struct Export {
    tests: Vec<Value>,
}

/// Parse a Datadog Synthetics export and map every recognisable entry
/// onto a `NewMonitor`. Returns the mapped list + a list of skipped
/// monitors (with reasons). Pure function — no I/O, no DB; the
/// integration test uses this directly without standing up Postgres.
pub fn parse_and_map(json: &str) -> Result<ImportPlan, ImportError> {
    let export: Export = serde_json::from_str(json)?;
    if export.tests.is_empty() {
        return Err(ImportError::NoMonitors);
    }

    let mut plan = ImportPlan::default();
    for raw in export.tests {
        match map_one(&raw) {
            Ok(m) => plan.mapped.push(m),
            Err(s) => {
                warn!(
                    source_name = %s.source_name,
                    source_kind = %s.source_kind,
                    reason = %s.reason,
                    "skip: unsupported datadog synthetic",
                );
                plan.skipped.push(s);
            }
        }
    }
    Ok(plan)
}

/// Map a single Datadog Synthetics test object onto a Rampart
/// `NewMonitor`. Returns the mapped form or a `SkippedMonitor`
/// describing why we couldn't translate this one.
fn map_one(raw: &Value) -> Result<MappedMonitor, SkippedMonitor> {
    let test_type = string_field(raw, "type").unwrap_or_else(|| "<missing>".to_string());
    let subtype = string_field(raw, "subtype").unwrap_or_default();
    // Compose a single "<type>/<subtype>" label so the skipped-list +
    // log lines surface the actual upstream shape that didn't map.
    let source_kind = if subtype.is_empty() {
        test_type.clone()
    } else {
        format!("{test_type}/{subtype}")
    };
    let source_name = string_field(raw, "name").unwrap_or_else(|| "<unnamed>".to_string());

    // Body-contains assertion presence flips an HTTP API test into the
    // Keyword probe so the imported monitor still asserts on the body.
    // See `docs/IMPORTERS.md` for the rationale.
    let has_body_contains = has_body_contains_assertion(raw);

    let mapped_kind = match map_kind(&test_type, &subtype, has_body_contains) {
        Some(k) => k,
        None => {
            let reason = format!("unsupported datadog synthetic `{source_kind}`");
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
    let name = source_name.clone();

    // Pull out the endpoint addressing fields. Datadog nests them all
    // under `config.request.*`.
    let request = raw.get("config").and_then(|c| c.get("request"));
    let url = request.and_then(|r| string_field(r, "url"));
    let hostname = request.and_then(|r| string_field(r, "host"));
    let port = request.and_then(|r| numeric_field(r, "port"));

    let http_method = request
        .and_then(|r| string_field(r, "method"))
        .map(|m| m.trim().to_ascii_uppercase())
        .unwrap_or_else(|| "GET".into());

    let timeout_seconds = request
        .and_then(|r| numeric_field(r, "timeout"))
        .unwrap_or(16)
        .clamp(1, 600);

    let options = raw.get("options");
    let interval_seconds = options
        .and_then(|o| numeric_field(o, "tick_every"))
        .unwrap_or(60)
        .clamp(10, 86400);
    let max_retries = options
        .and_then(|o| o.get("retry"))
        .and_then(|r| numeric_field(r, "count"))
        .unwrap_or(0)
        .max(0);

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

/// Datadog `(type, subtype)` -> Rampart probe kind. Returns `None` for
/// shapes Rampart has no equivalent for (multi-step synthetics, unknown
/// future subtypes); those get reported as skipped.
fn map_kind(test_type: &str, subtype: &str, has_body_contains: bool) -> Option<MonitorKind> {
    // Normalise case — Datadog ships these lowercase but be defensive
    // against capitalisation drift across API revisions.
    let t = test_type.to_ascii_lowercase();
    let s = subtype.to_ascii_lowercase();
    let kind = match (t.as_str(), s.as_str()) {
        ("api", "http") => {
            if has_body_contains {
                MonitorKind::Keyword
            } else {
                MonitorKind::Http
            }
        }
        ("api", "tcp") => MonitorKind::Tcp,
        ("api", "dns") => MonitorKind::Dns,
        ("api", "ssl") => MonitorKind::Tls,
        ("api", "icmp") => MonitorKind::Ping,
        ("api", "grpc") => MonitorKind::Grpc,
        ("api", "websocket") => MonitorKind::Websocket,
        // UDP has no native Rampart probe — TCP is the closest connection-
        // primitive we ship. Documented as imperfect in docs/IMPORTERS.md.
        ("api", "udp") => MonitorKind::Tcp,
        // Multi-step synthetics chain several requests; Rampart does not
        // model this. Skip with a warning so the operator can hand-port.
        ("api", "multi") => return None,
        ("browser", _) => MonitorKind::Browser,
        _ => return None,
    };
    Some(kind)
}

/// Does this test's `config.assertions` array contain a body-contains
/// assertion (i.e. `{"type": "body", "operator": "contains", …}`)?
/// Presence of one signals that the imported HTTP monitor should be
/// promoted to a Keyword probe so the body assertion isn't silently
/// dropped.
fn has_body_contains_assertion(raw: &Value) -> bool {
    let Some(assertions) = raw
        .get("config")
        .and_then(|c| c.get("assertions"))
        .and_then(|a| a.as_array())
    else {
        return false;
    };
    assertions.iter().any(|a| {
        let ty = a.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let op = a.get("operator").and_then(|v| v.as_str()).unwrap_or("");
        ty.eq_ignore_ascii_case("body") && op.eq_ignore_ascii_case("contains")
    })
}

/// Pull a string out of the JSON object. Accepts both `"foo"` and `null`
/// (returns `None` for null), and tolerates a missing key.
fn string_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

/// Pull a numeric-ish field out of the JSON. Datadog ships numbers as
/// bare JSON numbers in the documented schema, but be defensive against
/// the occasional stringified field (older API revisions); we accept
/// both so the importer doesn't choke on either flavour of dump.
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
    fn maps_http_and_http_with_body_contains() {
        let json = r#"{"tests":[
            {"name": "Plain HTTP", "type": "api", "subtype": "http",
             "config": {"request": {"url": "https://a.example.com"}}},
            {"name": "Keyword HTTP", "type": "api", "subtype": "http",
             "config": {
               "request": {"url": "https://b.example.com"},
               "assertions": [{"type": "body", "operator": "contains", "target": "OK"}]
             }}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped.len(), 2);
        assert_eq!(plan.mapped[0].mapped_kind, MonitorKind::Http);
        assert_eq!(plan.mapped[1].mapped_kind, MonitorKind::Keyword);
        assert!(plan.skipped.is_empty());
    }

    #[test]
    fn maps_tcp_and_icmp_with_default_interval() {
        let json = r#"{"tests":[
            {"name": "TCP", "type": "api", "subtype": "tcp",
             "config": {"request": {"host": "db", "port": 5432}}},
            {"name": "Box", "type": "api", "subtype": "icmp",
             "config": {"request": {"host": "10.0.0.1"}}}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped.len(), 2);
        assert_eq!(plan.mapped[0].mapped_kind, MonitorKind::Tcp);
        assert_eq!(plan.mapped[0].new_monitor.port, Some(5432));
        assert_eq!(plan.mapped[1].mapped_kind, MonitorKind::Ping);
        // No tick_every in the fixture -> default 60s applies.
        assert_eq!(plan.mapped[1].new_monitor.interval_seconds, 60);
    }

    #[test]
    fn skips_multi_and_unknown() {
        let json = r#"{"tests":[
            {"name": "MultiStep", "type": "api", "subtype": "multi",
             "config": {"request": {}}},
            {"name": "Mystery", "type": "api", "subtype": "smtp",
             "config": {"request": {}}}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert!(plan.mapped.is_empty());
        assert_eq!(plan.skipped.len(), 2);
        assert!(plan.skipped[0].reason.contains("api/multi"));
        assert!(plan.skipped[1].reason.contains("api/smtp"));
    }

    #[test]
    fn browser_maps_to_browser_regardless_of_subtype() {
        let json = r#"{"tests":[
            {"name": "Login flow", "type": "browser",
             "config": {"request": {"url": "https://x"}}}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].mapped_kind, MonitorKind::Browser);
    }

    #[test]
    fn udp_maps_to_tcp_as_documented() {
        let json = r#"{"tests":[
            {"name": "udp probe", "type": "api", "subtype": "udp",
             "config": {"request": {"host": "h", "port": 5060}}}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].mapped_kind, MonitorKind::Tcp);
        assert_eq!(plan.mapped[0].new_monitor.port, Some(5060));
    }

    #[test]
    fn tick_every_accepts_string_or_number() {
        let json = r#"{"tests":[
            {"name": "a", "type": "api", "subtype": "http",
             "config": {"request": {"url": "https://a"}},
             "options": {"tick_every": 120}},
            {"name": "b", "type": "api", "subtype": "http",
             "config": {"request": {"url": "https://b"}},
             "options": {"tick_every": "300"}}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].new_monitor.interval_seconds, 120);
        assert_eq!(plan.mapped[1].new_monitor.interval_seconds, 300);
    }

    #[test]
    fn retry_count_translates_to_max_retries() {
        let json = r#"{"tests":[
            {"name": "a", "type": "api", "subtype": "http",
             "config": {"request": {"url": "https://a"}},
             "options": {"retry": {"count": 3}}}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].new_monitor.max_retries, 3);
    }

    #[test]
    fn missing_tests_array_errors() {
        let json = r#"{"data": []}"#;
        let err = parse_and_map(json).unwrap_err();
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
        let json = r#"{"tests":[
            {"name": "fast", "type": "api", "subtype": "http",
             "config": {"request": {"url": "https://x"}},
             "options": {"tick_every": 5}},
            {"name": "slow", "type": "api", "subtype": "http",
             "config": {"request": {"url": "https://y"}},
             "options": {"tick_every": 99999}}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].new_monitor.interval_seconds, 10);
        assert_eq!(plan.mapped[1].new_monitor.interval_seconds, 86400);
    }
}
