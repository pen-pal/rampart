//! Pingdom monitor export importer.
//!
//! Reads a Pingdom `GET /api/3.1/checks` JSON dump — operators capture
//! one with their own API token, save it to disk, hand the path to
//! `rampart-import pingdom`. We don't reach out to the Pingdom API
//! ourselves: importers are one-shot, offline tools by design (see
//! `docs/IMPORTERS.md` + `CONTRIBUTING.md` "Importers" bullet).
//!
//! ## Mapping
//!
//! See the table in `docs/IMPORTERS.md`. Anything not in the table is
//! skipped with a `tracing::warn!` line so the operator can hand-port
//! the unusual monitors (multi-step transactions, etc.) after the
//! import.
//!
//! ## Field translation
//!
//! | Pingdom                                | Rampart `NewMonitor`            |
//! | -------------------------------------- | ------------------------------- |
//! | `name`                                 | `name`                          |
//! | `hostname` + `port` + `encryption`     | `url` (for HTTP-shape probes —  |
//! | + `url` (path)                         | reconstructed as a full URL)    |
//! | `hostname` (+ `port`)                  | `hostname` / `port` (for        |
//! |                                        | TCP-shape probes)               |
//! | `resolution` (minutes)                 | `interval_seconds = max(60,     |
//! |                                        | resolution * 60)`               |
//! | `verify_certificate` (`false`)         | `ignore_tls` (`true`)           |
//! | `should_contain`                       | `config["keyword"]` (and flips  |
//! |                                        | `Http` → `Keyword`)             |
//! | `http_method`                          | `http_method` (upper-cased)     |
//!
//! Pingdom ships `resolution` in **minutes** (1, 5, 15, 30, or 60) —
//! a coarser cadence than Rampart's per-second interval. We multiply by
//! 60 and floor at 60 seconds so the resulting monitor still satisfies
//! `NewMonitor`'s `interval_seconds >= 10` validation rule.
//!
//! `udp` checks are mapped onto `Tcp` because Rampart has no UDP probe
//! today; the operator gets a `tracing::warn!` line about the gap so
//! they know the imported monitor is testing connect-ability over the
//! wrong transport and should be hand-ported (or removed) post-import.

use serde::Deserialize;
use serde_json::Value;
use tracing::warn;

use rampart_core::monitor::NewMonitor;
use rampart_core::MonitorKind;

pub use super::ImportError;
use super::{ImportPlan, MappedMonitor, SkippedMonitor};

/// The minimal shape of the top-level Pingdom export. Real exports
/// carry dozens more fields per check; we deserialise into
/// `serde_json::Value` so we can pick out only the ones we need and
/// ignore the rest without listing every possible variant.
#[derive(Debug, Deserialize)]
struct Export {
    checks: Vec<Value>,
}

/// Parse a Pingdom export and map every recognisable entry onto a
/// `NewMonitor`. Returns the mapped list + a list of skipped checks
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
                    "skip: unsupported pingdom check",
                );
                plan.skipped.push(s);
            }
        }
    }
    Ok(plan)
}

/// Map a single Pingdom check object onto a Rampart `NewMonitor`.
/// Returns the mapped form or a `SkippedMonitor` describing why we
/// couldn't translate this one.
fn map_one(raw: &Value) -> Result<MappedMonitor, SkippedMonitor> {
    let source_kind = string_field(raw, "type").unwrap_or_else(|| "<missing>".to_string());
    let source_name = string_field(raw, "name").unwrap_or_else(|| "<unnamed>".to_string());

    // `should_contain` flips an HTTP-shape probe into a Keyword check.
    // We also stash the keyword into `config["keyword"]` so the probe
    // implementation has it available at run-time.
    let should_contain = string_field(raw, "should_contain");
    let has_keyword = should_contain
        .as_deref()
        .map(|s| !s.is_empty())
        .unwrap_or(false);

    let mapped_kind = match map_kind(&source_kind, has_keyword) {
        Some(k) => k,
        None => {
            let reason = if source_kind.eq_ignore_ascii_case("transaction") {
                "pingdom multi-step transaction has no Rampart equivalent".to_string()
            } else {
                format!("unsupported pingdom type `{source_kind}`")
            };
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

    // Pingdom ships `resolution` in minutes (1, 5, 15, 30, 60). Convert
    // to seconds for Rampart, floor at 60s — Rampart's `interval_seconds`
    // is per-second so we don't need to clamp on the high end (60 mins =
    // 3600 s, well under the 86400 cap), but the validation lower bound
    // is 10 s and a Pingdom resolution of 0 would slip through without
    // the floor.
    let resolution_minutes = numeric_field(raw, "resolution").unwrap_or(1);
    let interval_seconds = (resolution_minutes.max(1) * 60).clamp(60, 86400);

    let verify_cert = bool_field(raw, "verify_certificate").unwrap_or(true);
    let encryption = bool_field(raw, "encryption").unwrap_or(false);
    let port = numeric_field(raw, "port");
    let hostname = string_field(raw, "hostname");
    let url_path = string_field(raw, "url");

    let (mut url_out, hostname_out, port_out) = match mapped_kind {
        // HTTP-shape probes: reconstruct a full URL from hostname +
        // port + encryption + path. Rampart's HTTP probe takes a single
        // URL string rather than the split fields Pingdom uses.
        MonitorKind::Http | MonitorKind::Keyword => {
            let url = build_http_url(hostname.as_deref(), port, encryption, url_path.as_deref());
            (url, None, None)
        }
        // Everything else is a network-primitive probe — keep the
        // split shape, drop the URL field.
        _ => (None, hostname.clone(), port),
    };

    // Per the field-translation contract: an empty-string url after URL
    // reconstruction means we couldn't reconstruct one (e.g. hostname
    // missing). Demote to None so the route layer's validation catches
    // it consistently.
    if url_out.as_deref() == Some("") {
        url_out = None;
    }

    // `should_contain` lives in the per-monitor `config` blob so the
    // Keyword probe can read it at runtime.
    let mut config = serde_json::Map::new();
    if let Some(kw) = should_contain.as_deref() {
        if !kw.is_empty() {
            config.insert("keyword".to_string(), Value::String(kw.to_string()));
        }
    }
    if let Some(neg) = string_field(raw, "should_not_contain") {
        if !neg.is_empty() {
            config.insert("keyword_negate".to_string(), Value::String(neg));
        }
    }

    let http_method = string_field(raw, "http_method")
        .map(|m| m.trim().to_ascii_uppercase())
        .unwrap_or_else(|| "GET".into());

    let new_monitor = NewMonitor {
        name: source_name.clone(),
        kind: mapped_kind,
        url: url_out,
        hostname: hostname_out,
        port: port_out,
        config: Value::Object(config),
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
        // Pingdom's `verify_certificate=false` is "don't verify the
        // cert" — Rampart's `ignore_tls=true` is the same thing.
        ignore_tls: !verify_cert,
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

/// Pingdom type string -> Rampart probe kind. Returns `None` for
/// types Rampart has no equivalent for (currently just `transaction`,
/// the multi-step synthetic browser flow); those get reported as
/// skipped.
fn map_kind(source: &str, has_keyword: bool) -> Option<MonitorKind> {
    let s = source.to_ascii_lowercase();
    let kind = match s.as_str() {
        // HTTP family: flip to Keyword when the operator asked for a
        // body assertion via `should_contain`.
        "http" | "httpcustom" => {
            if has_keyword {
                MonitorKind::Keyword
            } else {
                MonitorKind::Http
            }
        }
        "tcp" => MonitorKind::Tcp,
        // UDP isn't a probe Rampart ships — closest match is Tcp.
        // Document the gap loudly so the operator hand-ports it.
        "udp" => {
            warn!(
                "pingdom UDP check mapped onto Tcp probe — Rampart has no UDP probe; review post-import"
            );
            MonitorKind::Tcp
        }
        "dns" => MonitorKind::Dns,
        "ping" => MonitorKind::Ping,
        "pop3" => MonitorKind::Pop3,
        "smtp" => MonitorKind::Smtp,
        "imap" => MonitorKind::Imap,
        "ssh" => MonitorKind::Ssh,
        // `transaction` (multi-step) and anything else: skip.
        _ => return None,
    };
    Some(kind)
}

/// Reconstruct a full HTTP(S) URL from Pingdom's split addressing
/// fields. Pingdom stores the hostname, the port, the TLS flag, and the
/// URL path separately; Rampart's HTTP probe wants the whole thing
/// glued together. Returns `None` when there's no hostname (the route
/// layer will reject the monitor on validation).
fn build_http_url(
    hostname: Option<&str>,
    port: Option<i32>,
    encryption: bool,
    path: Option<&str>,
) -> Option<String> {
    let host = hostname?;
    let scheme = if encryption { "https" } else { "http" };
    let default_port = if encryption { 443 } else { 80 };
    let path = path.unwrap_or("/");
    // Pingdom stores the path with a leading slash; tolerate both
    // shapes so we don't emit `//` or `host` when the operator's
    // export trimmed it.
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    let url = match port {
        Some(p) if p != default_port => format!("{scheme}://{host}:{p}{path}"),
        _ => format!("{scheme}://{host}{path}"),
    };
    Some(url)
}

/// Pull a string out of the JSON object. Accepts both `"foo"` and
/// `null` (returns `None` for null), and tolerates a missing key.
fn string_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

/// Pull a numeric-ish field out of the JSON. Pingdom's API ships
/// numbers as bare JSON numbers in the documented shape, but we also
/// tolerate numbers-as-strings (`"60"`) to match the leniency the
/// Site24x7 importer uses — operators sometimes hand-edit the dump.
fn numeric_field(v: &Value, key: &str) -> Option<i32> {
    match v.get(key)? {
        Value::Number(n) => n.as_i64().map(|x| x as i32),
        Value::String(s) => s.trim().parse::<i32>().ok(),
        _ => None,
    }
}

/// Pull a bool out of the JSON object. Tolerates bare `true` / `false`
/// and the string forms (`"true"` / `"false"`) the same way.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_http_and_http_with_keyword() {
        let json = r#"{"checks":[
            {"id": 1, "name": "Plain HTTP", "type": "http", "hostname": "a.example.com",
             "resolution": 1, "encryption": true, "url": "/health"},
            {"id": 2, "name": "Keyword HTTP", "type": "http", "hostname": "b.example.com",
             "resolution": 5, "encryption": true, "url": "/", "should_contain": "OK"}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped.len(), 2);
        assert_eq!(plan.mapped[0].mapped_kind, MonitorKind::Http);
        assert_eq!(plan.mapped[1].mapped_kind, MonitorKind::Keyword);
        assert!(plan.skipped.is_empty());
    }

    #[test]
    fn skips_transaction_kind() {
        let json = r#"{"checks":[
            {"id": 3, "name": "Login flow", "type": "transaction",
             "resolution": 5}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert!(plan.mapped.is_empty());
        assert_eq!(plan.skipped.len(), 1);
        assert!(plan.skipped[0].reason.contains("transaction"));
    }

    #[test]
    fn udp_maps_onto_tcp_with_warning() {
        let json = r#"{"checks":[
            {"id": 4, "name": "DNS over UDP", "type": "udp",
             "hostname": "10.0.0.1", "port": 53, "resolution": 1}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped.len(), 1);
        assert_eq!(plan.mapped[0].mapped_kind, MonitorKind::Tcp);
        assert_eq!(plan.mapped[0].new_monitor.port, Some(53));
    }

    #[test]
    fn resolution_minutes_to_seconds() {
        let json = r#"{"checks":[
            {"id": 5, "name": "fast", "type": "http", "hostname": "x",
             "resolution": 1, "encryption": false, "url": "/"},
            {"id": 6, "name": "slow", "type": "http", "hostname": "y",
             "resolution": 60, "encryption": false, "url": "/"}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].new_monitor.interval_seconds, 60);
        assert_eq!(plan.mapped[1].new_monitor.interval_seconds, 3600);
    }

    #[test]
    fn verify_certificate_false_sets_ignore_tls() {
        let json = r#"{"checks":[
            {"id": 7, "name": "selfsigned", "type": "http", "hostname": "self.example",
             "resolution": 1, "encryption": true, "url": "/",
             "verify_certificate": false}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert!(plan.mapped[0].new_monitor.ignore_tls);
    }

    #[test]
    fn http_url_reconstruction_uses_scheme_and_port() {
        // encryption=true + default 443 -> https://host/path (no :443)
        // encryption=false + custom 8080 -> http://host:8080/path
        let json = r#"{"checks":[
            {"id": 8, "name": "tls-default", "type": "http", "hostname": "a.example.com",
             "resolution": 1, "encryption": true, "port": 443, "url": "/health"},
            {"id": 9, "name": "plain-custom", "type": "http", "hostname": "b.example.com",
             "resolution": 1, "encryption": false, "port": 8080, "url": "/status"}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(
            plan.mapped[0].new_monitor.url.as_deref(),
            Some("https://a.example.com/health")
        );
        assert_eq!(
            plan.mapped[1].new_monitor.url.as_deref(),
            Some("http://b.example.com:8080/status")
        );
    }

    #[test]
    fn tcp_keeps_hostname_and_port_split() {
        let json = r#"{"checks":[
            {"id": 10, "name": "redis", "type": "tcp", "hostname": "cache.example.com",
             "port": 6379, "resolution": 1}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].mapped_kind, MonitorKind::Tcp);
        assert_eq!(plan.mapped[0].new_monitor.url, None);
        assert_eq!(
            plan.mapped[0].new_monitor.hostname.as_deref(),
            Some("cache.example.com")
        );
        assert_eq!(plan.mapped[0].new_monitor.port, Some(6379));
    }

    #[test]
    fn should_contain_lands_in_config() {
        let json = r#"{"checks":[
            {"id": 11, "name": "kw", "type": "http", "hostname": "a", "resolution": 1,
             "encryption": false, "url": "/", "should_contain": "Welcome"}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].mapped_kind, MonitorKind::Keyword);
        let kw = plan.mapped[0]
            .new_monitor
            .config
            .get("keyword")
            .and_then(|v| v.as_str());
        assert_eq!(kw, Some("Welcome"));
    }

    #[test]
    fn http_method_uppercased() {
        let json = r#"{"checks":[
            {"id": 12, "name": "post", "type": "http", "hostname": "a", "resolution": 1,
             "encryption": false, "url": "/", "http_method": "post"}
        ]}"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].new_monitor.http_method, "POST");
    }

    #[test]
    fn missing_checks_array_errors() {
        let json = r#"{"data": []}"#;
        let err = parse_and_map(json).unwrap_err();
        assert!(matches!(
            err,
            ImportError::Parse(_) | ImportError::NoMonitors
        ));
    }
}
