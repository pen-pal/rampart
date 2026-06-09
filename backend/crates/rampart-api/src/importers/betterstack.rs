//! BetterStack (Uptime) monitor export importer.
//!
//! Reads a BetterStack `GET /api/v2/monitors` JSON dump — operators
//! capture one with their own API token, save it to disk, hand the
//! path to `rampart-import betterstack`. We don't reach out to the
//! BetterStack API ourselves: importers are one-shot, offline tools by
//! design (see `docs/IMPORTERS.md` + `CONTRIBUTING.md` "Importers"
//! bullet).
//!
//! ## Mapping
//!
//! BetterStack ships the response as `{"data":[{"id":"…","attributes":
//! {...}}, …]}`. Each `attributes` object carries the actual monitor
//! shape — most importantly `monitor_type`, which selects the probe
//! family. See the table in `docs/IMPORTERS.md`. Anything not in the
//! table is skipped with a `tracing::warn!` line so the operator can
//! hand-port the unusual monitors after the import.
//!
//! ## Field translation
//!
//! | BetterStack                            | Rampart `NewMonitor`            |
//! | -------------------------------------- | ------------------------------- |
//! | `pronounceable_name`                   | `name` (required)               |
//! | `url`                                  | `url`                           |
//! | `port`                                 | `port` (i32)                    |
//! | `request_method`                       | `http_method` (uppercased)      |
//! | `request_headers`                      | `http_headers`                  |
//! | `request_body`                         | `http_body`                     |
//! | `expected_status_codes`                | `accepted_statuses`             |
//! | `required_keyword`                     | `config["keyword"]` (kept for both keyword + keyword_absence kinds) |
//! | `check_frequency` (seconds)            | `interval_seconds`              |
//! | `request_timeout` (seconds)            | `timeout_seconds`               |
//! | `follow_redirects`                     | `follow_redirect`               |
//! | `verify_ssl` (`false`)                 | `ignore_tls` (`true`)           |
//!
//! BetterStack ships `check_frequency` / `request_timeout` as bare JSON
//! numbers per the documented schema, but we accept stringified numbers
//! defensively (some older API revisions stringified them). Missing /
//! unparseable falls back to `NewMonitor`'s serde defaults (60s
//! interval, 16s timeout).

use serde::Deserialize;
use serde_json::{Map, Value};
use tracing::warn;

use rampart_core::monitor::NewMonitor;
use rampart_core::MonitorKind;

pub use super::ImportError;
use super::{ImportPlan, MappedMonitor, SkippedMonitor};

/// The minimal shape of the top-level BetterStack export. Real exports
/// carry many more fields per monitor; we deserialise into
/// `serde_json::Value` so we can pick out only the ones we need and
/// ignore the rest without listing every possible variant.
#[derive(Debug, Deserialize)]
struct Export {
    data: Vec<Value>,
}

/// Parse a BetterStack export and map every recognisable entry onto a
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
                    "skip: unsupported betterstack monitor",
                );
                plan.skipped.push(s);
            }
        }
    }
    Ok(plan)
}

/// Map a single BetterStack monitor onto a Rampart `NewMonitor`.
/// Returns the mapped form or a `SkippedMonitor` describing why we
/// couldn't translate this one. The BetterStack envelope wraps the
/// actual shape under `attributes`; everything else (id, type,
/// relationships) is dropped.
fn map_one(raw: &Value) -> Result<MappedMonitor, SkippedMonitor> {
    let attrs = raw.get("attributes").unwrap_or(raw);

    let monitor_type =
        string_field(attrs, "monitor_type").unwrap_or_else(|| "<missing>".to_string());
    let source_kind = monitor_type.clone();
    let source_name =
        string_field(attrs, "pronounceable_name").unwrap_or_else(|| "<unnamed>".to_string());

    let mapped_kind = match map_kind(&monitor_type) {
        Some(k) => k,
        None => {
            let reason = format!("unsupported betterstack monitor_type `{source_kind}`");
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
            reason: "missing pronounceable_name".into(),
        });
    }

    // BetterStack stores the endpoint addressing in `url`. For probe
    // kinds Rampart addresses by host+port (Tcp, Smtp, Pop3, Imap,
    // Dns), we keep the URL too — it's harmless extra context, and
    // operators can clean it up at edit time. Port is a top-level
    // field on BetterStack's shape.
    let url = string_field(attrs, "url");
    let port = numeric_field(attrs, "port");

    let http_method = string_field(attrs, "request_method")
        .map(|m| m.trim().to_ascii_uppercase())
        .unwrap_or_else(|| "GET".into());

    let http_body = string_field(attrs, "request_body").filter(|s| !s.is_empty());
    let http_headers = attrs.get("request_headers").cloned().and_then(|v| {
        // Drop empty arrays / objects so the column reads cleanly.
        match &v {
            Value::Array(a) if a.is_empty() => None,
            Value::Object(o) if o.is_empty() => None,
            Value::Null => None,
            _ => Some(v),
        }
    });

    let accepted_statuses =
        numeric_array(attrs, "expected_status_codes").unwrap_or_else(default_accepted_statuses);

    // Keyword + keyword_absence both carry the substring in
    // `required_keyword`. The absence-flavour is documented as
    // imperfect — Rampart's Keyword probe only asserts presence; the
    // operator needs to flip the resulting monitor to `upside_down` (or
    // add a `keyword_absence` flag in config) by hand post-import.
    let mut config_obj = Map::new();
    if let Some(kw) = string_field(attrs, "required_keyword") {
        if !kw.is_empty() {
            config_obj.insert("keyword".to_string(), Value::String(kw));
        }
    }
    if monitor_type.eq_ignore_ascii_case("keyword_absence") {
        config_obj.insert("keyword_absence".to_string(), Value::Bool(true));
    }
    let config = Value::Object(config_obj);

    let interval_seconds = numeric_field(attrs, "check_frequency")
        .unwrap_or(60)
        .clamp(10, 86400);
    let timeout_seconds = numeric_field(attrs, "request_timeout")
        .unwrap_or(16)
        .clamp(1, 600);

    let follow_redirect = bool_field(attrs, "follow_redirects").unwrap_or(true);
    let verify_ssl = bool_field(attrs, "verify_ssl").unwrap_or(true);

    let new_monitor = NewMonitor {
        name: source_name.clone(),
        kind: mapped_kind,
        url,
        hostname: None,
        port,
        config,
        interval_seconds,
        timeout_seconds,
        max_retries: 0,
        retry_interval_sec: 60,
        resend_interval_sec: 0,
        upside_down: false,
        http_method,
        http_body,
        http_headers,
        accepted_statuses,
        follow_redirect,
        ignore_tls: !verify_ssl,
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

/// BetterStack `monitor_type` -> Rampart probe kind. Returns `None` for
/// shapes Rampart has no equivalent for (unknown future kinds); those
/// get reported as skipped.
fn map_kind(monitor_type: &str) -> Option<MonitorKind> {
    let s = monitor_type.to_ascii_lowercase();
    let kind = match s.as_str() {
        "status" => MonitorKind::Http,
        "keyword" => MonitorKind::Keyword,
        // BetterStack's "keyword_absence" asserts a string is *not*
        // present. Rampart's Keyword probe only asserts presence today;
        // we flag the imported monitor with `config["keyword_absence"]`
        // = true so a future probe revision can pick it up, but the
        // current runtime treats it as a regular Keyword check. See
        // docs/IMPORTERS.md for the manual-port note.
        "keyword_absence" => MonitorKind::Keyword,
        "ping" => MonitorKind::Ping,
        "tcp" => MonitorKind::Tcp,
        // UDP has no native Rampart probe — TCP is the closest
        // connection-primitive we ship. Documented as imperfect in
        // docs/IMPORTERS.md.
        "udp" => MonitorKind::Tcp,
        "dns" => MonitorKind::Dns,
        "smtp" => MonitorKind::Smtp,
        "pop" | "pop3" => MonitorKind::Pop3,
        "imap" => MonitorKind::Imap,
        "playwright" => MonitorKind::Browser,
        _ => return None,
    };
    Some(kind)
}

/// Pull a string out of the JSON object. Accepts both `"foo"` and `null`
/// (returns `None` for null), and tolerates a missing key.
fn string_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

/// Pull a numeric-ish field out of the JSON. BetterStack ships numbers
/// as bare JSON numbers in the documented schema, but accept
/// stringified numbers defensively (older API revisions occasionally
/// stringified them).
fn numeric_field(v: &Value, key: &str) -> Option<i32> {
    match v.get(key)? {
        Value::Number(n) => n.as_i64().map(|x| x as i32),
        Value::String(s) => s.trim().parse::<i32>().ok(),
        _ => None,
    }
}

/// Pull a bool out of the JSON object. Tolerates bare `true` / `false`
/// and the string forms (`"true"` / `"false"`).
fn bool_field(v: &Value, key: &str) -> Option<bool> {
    match v.get(key)? {
        Value::Bool(b) => Some(*b),
        Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// Pull a `Vec<i32>` out of the JSON object. Used for the
/// `expected_status_codes` field which BetterStack ships as an array of
/// integers. Tolerates stringified numbers per the same leniency the
/// rest of the importer applies.
fn numeric_array(v: &Value, key: &str) -> Option<Vec<i32>> {
    let arr = v.get(key)?.as_array()?;
    if arr.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let n = match item {
            Value::Number(n) => n.as_i64().map(|x| x as i32),
            Value::String(s) => s.trim().parse::<i32>().ok(),
            _ => None,
        };
        if let Some(n) = n {
            out.push(n);
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn default_accepted_statuses() -> Vec<i32> {
    vec![200, 201, 202, 203, 204, 205, 206, 207, 208, 226]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_status_and_keyword_and_keyword_absence() {
        let json = r#"{"data":[
            {"id": "1", "attributes": {
                "monitor_type": "status",
                "pronounceable_name": "Plain HTTP",
                "url": "https://a.example.com",
                "check_frequency": 60
            }},
            {"id": "2", "attributes": {
                "monitor_type": "keyword",
                "pronounceable_name": "Body check",
                "url": "https://b.example.com",
                "required_keyword": "OK",
                "check_frequency": 120
            }},
            {"id": "3", "attributes": {
                "monitor_type": "keyword_absence",
                "pronounceable_name": "No errors",
                "url": "https://c.example.com",
                "required_keyword": "ERROR",
                "check_frequency": 60
            }}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped.len(), 3);
        assert_eq!(plan.mapped[0].mapped_kind, MonitorKind::Http);
        assert_eq!(plan.mapped[1].mapped_kind, MonitorKind::Keyword);
        assert_eq!(plan.mapped[1].new_monitor.config["keyword"], "OK");
        assert_eq!(plan.mapped[2].mapped_kind, MonitorKind::Keyword);
        assert_eq!(plan.mapped[2].new_monitor.config["keyword"], "ERROR");
        assert_eq!(plan.mapped[2].new_monitor.config["keyword_absence"], true);
    }

    #[test]
    fn ping_tcp_dns_smtp_pop_imap_each_map_correctly() {
        let json = r#"{"data":[
            {"id":"1","attributes":{"monitor_type":"ping","pronounceable_name":"Ping1","url":"10.0.0.1"}},
            {"id":"2","attributes":{"monitor_type":"tcp","pronounceable_name":"TCP1","url":"db.example.com","port":5432}},
            {"id":"3","attributes":{"monitor_type":"dns","pronounceable_name":"DNS1","url":"example.com"}},
            {"id":"4","attributes":{"monitor_type":"smtp","pronounceable_name":"SMTP1","url":"smtp.example.com","port":25}},
            {"id":"5","attributes":{"monitor_type":"pop","pronounceable_name":"POP1","url":"pop.example.com","port":110}},
            {"id":"6","attributes":{"monitor_type":"imap","pronounceable_name":"IMAP1","url":"imap.example.com","port":143}}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].mapped_kind, MonitorKind::Ping);
        assert_eq!(plan.mapped[1].mapped_kind, MonitorKind::Tcp);
        assert_eq!(plan.mapped[2].mapped_kind, MonitorKind::Dns);
        assert_eq!(plan.mapped[3].mapped_kind, MonitorKind::Smtp);
        assert_eq!(plan.mapped[4].mapped_kind, MonitorKind::Pop3);
        assert_eq!(plan.mapped[5].mapped_kind, MonitorKind::Imap);
        assert_eq!(plan.mapped[3].new_monitor.port, Some(25));
    }

    #[test]
    fn udp_falls_back_to_tcp_as_documented() {
        let json = r#"{"data":[
            {"id":"1","attributes":{"monitor_type":"udp","pronounceable_name":"UDP1","url":"h","port":5060}}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].mapped_kind, MonitorKind::Tcp);
    }

    #[test]
    fn playwright_maps_to_browser() {
        let json = r#"{"data":[
            {"id":"1","attributes":{"monitor_type":"playwright","pronounceable_name":"Login","url":"https://x"}}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].mapped_kind, MonitorKind::Browser);
    }

    #[test]
    fn skips_unknown_monitor_type() {
        let json = r#"{"data":[
            {"id":"1","attributes":{"monitor_type":"mystery","pronounceable_name":"Future kind"}}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert!(plan.mapped.is_empty());
        assert_eq!(plan.skipped.len(), 1);
        assert_eq!(plan.skipped[0].source_kind, "mystery");
    }

    #[test]
    fn verify_ssl_false_sets_ignore_tls() {
        let json = r#"{"data":[
            {"id":"1","attributes":{"monitor_type":"status","pronounceable_name":"Self-signed",
                "url":"https://self.example","verify_ssl":false}}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert!(plan.mapped[0].new_monitor.ignore_tls);
    }

    #[test]
    fn expected_status_codes_translates_to_accepted_statuses() {
        let json = r#"{"data":[
            {"id":"1","attributes":{"monitor_type":"status","pronounceable_name":"strict",
                "url":"https://x","expected_status_codes":[200,204]}}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].new_monitor.accepted_statuses, vec![200, 204]);
    }

    #[test]
    fn interval_clamped_to_validation_range() {
        let json = r#"{"data":[
            {"id":"1","attributes":{"monitor_type":"status","pronounceable_name":"fast",
                "url":"https://x","check_frequency":5}},
            {"id":"2","attributes":{"monitor_type":"status","pronounceable_name":"slow",
                "url":"https://y","check_frequency":99999}}
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
}
