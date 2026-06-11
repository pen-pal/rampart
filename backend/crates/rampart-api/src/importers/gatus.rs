//! Gatus endpoint importer.
//!
//! Reads a Gatus `GET /api/v1/endpoints/statuses` JSON dump — operators
//! capture one with their own credentials, save it to disk, hand the
//! path to `rampart-import gatus`. We don't reach out to the Gatus API
//! ourselves: importers are one-shot, offline tools by design (see
//! `docs/IMPORTERS.md` + `CONTRIBUTING.md` "Importers" bullet).
//!
//! ## Mapping
//!
//! Gatus is configured via YAML but exposes the live endpoint roster as
//! JSON. The status endpoint returns a **bare JSON array** of endpoint
//! objects, each carrying a `url` whose scheme picks the probe family:
//!
//! | `url` scheme              | Rampart `MonitorKind` |
//! | ------------------------- | --------------------- |
//! | `http://` / `https://`    | `Http`                |
//! | `tcp://host:port`         | `Tcp`                 |
//! | `icmp://host`             | `Ping`                |
//! | `dns://…`                 | `Dns`                 |
//! | `starttls://` / `tls://`  | `Tls`                 |
//!
//! Anything else (a scheme Gatus may add later) is skipped with a
//! `tracing::warn!` line so the operator can hand-port it.
//!
//! ## Field translation
//!
//! | Gatus                  | Rampart `NewMonitor`                                           |
//! | ---------------------- | -------------------------------------------------------------- |
//! | `name`                 | `name` (prefixed `group/name` when `group` is present)         |
//! | `group`                | (folded into `name`)                                           |
//! | `url`                  | `url` for `Http`; `hostname` (+ `port`) for `tcp://`; otherwise the scheme-stripped host |
//! | (no interval)          | `interval_seconds` = `60` (the status export carries no cadence) |

use serde_json::Value;
use tracing::warn;

use rampart_core::monitor::NewMonitor;
use rampart_core::MonitorKind;

pub use super::ImportError;
use super::{ImportPlan, MappedMonitor, SkippedMonitor};

/// Parse a Gatus endpoint-statuses export and map every recognisable
/// entry onto a `NewMonitor`. Returns the mapped list + a list of
/// skipped entries (with reasons). Pure function — no I/O, no DB; the
/// integration test uses this directly without standing up Postgres.
///
/// The Gatus status endpoint returns a bare JSON array at the top level
/// (not a `{"data":[…]}` envelope), so we deserialise straight into
/// `Vec<Value>`.
pub fn parse_and_map(json: &str) -> Result<ImportPlan, ImportError> {
    let endpoints: Vec<Value> = serde_json::from_str(json)?;
    if endpoints.is_empty() {
        return Err(ImportError::NoMonitors);
    }

    let mut plan = ImportPlan::default();
    for raw in endpoints {
        match map_one(&raw) {
            Ok(m) => plan.mapped.push(m),
            Err(s) => {
                warn!(
                    source_name = %s.source_name,
                    source_kind = %s.source_kind,
                    reason = %s.reason,
                    "skip: unsupported gatus endpoint",
                );
                plan.skipped.push(s);
            }
        }
    }
    Ok(plan)
}

/// Map a single Gatus endpoint onto a Rampart `NewMonitor`. Returns the
/// mapped form or a `SkippedMonitor` describing why we couldn't
/// translate this one.
fn map_one(raw: &Value) -> Result<MappedMonitor, SkippedMonitor> {
    let url = string_field(raw, "url").filter(|s| !s.is_empty());
    let group = string_field(raw, "group").filter(|s| !s.is_empty());
    let name = string_field(raw, "name").filter(|s| !s.is_empty());

    // Compose the display name: prefix with the group when present so
    // operators who organised endpoints into groups keep that context
    // ("Core/API" rather than a bare "API").
    let source_name = match (&group, &name) {
        (Some(g), Some(n)) => format!("{g}/{n}"),
        (None, Some(n)) => n.clone(),
        _ => "<unnamed>".to_string(),
    };

    // `source_kind` is the URL scheme — useful in the skip report so the
    // operator can see which protocol we couldn't translate.
    let scheme = url.as_deref().and_then(scheme_of).unwrap_or_default();
    let source_kind = if scheme.is_empty() {
        "<no-scheme>".to_string()
    } else {
        scheme.clone()
    };

    if name.is_none() {
        return Err(SkippedMonitor {
            source_name,
            source_kind,
            reason: "missing name".into(),
        });
    }

    let url = match url {
        Some(u) => u,
        None => {
            return Err(SkippedMonitor {
                source_name,
                source_kind,
                reason: "missing url".into(),
            });
        }
    };

    let mapped_kind = match map_kind(&scheme) {
        Some(k) => k,
        None => {
            return Err(SkippedMonitor {
                source_name,
                source_kind,
                reason: format!("unsupported gatus url scheme `{scheme}`"),
            });
        }
    };

    // For HTTP probes we keep the full URL. For everything else Rampart
    // probes a host (+ port for TCP), so we strip the scheme and split
    // off the port.
    let (mon_url, hostname, port) = match mapped_kind {
        MonitorKind::Http => (Some(url), None, None),
        MonitorKind::Tcp => {
            let (host, port) = split_host_port(strip_scheme(&url));
            (None, Some(host), port)
        }
        _ => (None, Some(strip_scheme(&url).to_string()), None),
    };

    let new_monitor = NewMonitor {
        name: source_name.clone(),
        kind: mapped_kind,
        url: mon_url,
        hostname,
        port,
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
        escalation_policy_id: None,
    };

    Ok(MappedMonitor {
        source_name,
        source_kind,
        mapped_kind,
        new_monitor,
    })
}

/// URL scheme -> Rampart probe kind. Returns `None` for schemes Rampart
/// has no equivalent for; those get reported as skipped.
fn map_kind(scheme: &str) -> Option<MonitorKind> {
    let kind = match scheme {
        "http" | "https" => MonitorKind::Http,
        "tcp" => MonitorKind::Tcp,
        "icmp" => MonitorKind::Ping,
        "dns" => MonitorKind::Dns,
        "starttls" | "tls" => MonitorKind::Tls,
        _ => return None,
    };
    Some(kind)
}

/// Lowercased scheme of a `scheme://rest` URL, or `None` when there's no
/// `://` separator.
fn scheme_of(url: &str) -> Option<String> {
    url.split_once("://")
        .map(|(scheme, _)| scheme.trim().to_ascii_lowercase())
}

/// The part of a `scheme://rest` URL after the `://`. Returns the whole
/// string unchanged when there's no scheme separator.
fn strip_scheme(url: &str) -> &str {
    url.split_once("://").map(|(_, rest)| rest).unwrap_or(url)
}

/// Split a `host:port` (already scheme-stripped) into host + optional
/// port. A trailing path is dropped. IPv6 literals are not modelled —
/// Gatus' `tcp://` form is `host:port`.
fn split_host_port(host_port: &str) -> (String, Option<i32>) {
    // Drop any path component after the host:port.
    let host_port = host_port.split('/').next().unwrap_or(host_port);
    match host_port.rsplit_once(':') {
        Some((host, port)) => match port.parse::<i32>() {
            Ok(p) => (host.to_string(), Some(p)),
            Err(_) => (host_port.to_string(), None),
        },
        None => (host_port.to_string(), None),
    }
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
    fn maps_each_scheme_to_correct_kind() {
        let json = r#"[
            {"name":"web","url":"https://example.com"},
            {"name":"db","url":"tcp://db.example.com:5432"},
            {"name":"router","url":"icmp://10.0.0.1"},
            {"name":"resolver","url":"dns://1.1.1.1"},
            {"name":"mail","url":"starttls://smtp.example.com:587"}
        ]"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped.len(), 5);
        assert_eq!(plan.mapped[0].mapped_kind, MonitorKind::Http);
        assert_eq!(plan.mapped[1].mapped_kind, MonitorKind::Tcp);
        assert_eq!(plan.mapped[2].mapped_kind, MonitorKind::Ping);
        assert_eq!(plan.mapped[3].mapped_kind, MonitorKind::Dns);
        assert_eq!(plan.mapped[4].mapped_kind, MonitorKind::Tls);
    }

    #[test]
    fn group_prefixes_name() {
        let json = r#"[
            {"name":"API","group":"Core","url":"https://api.example.com"}
        ]"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].new_monitor.name, "Core/API");
    }

    #[test]
    fn no_group_keeps_bare_name() {
        let json = r#"[
            {"name":"API","url":"https://api.example.com"}
        ]"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].new_monitor.name, "API");
    }

    #[test]
    fn http_keeps_url() {
        let json = r#"[
            {"name":"web","url":"https://example.com/health"}
        ]"#;
        let plan = parse_and_map(json).unwrap();
        let m = &plan.mapped[0];
        assert_eq!(
            m.new_monitor.url.as_deref(),
            Some("https://example.com/health")
        );
        assert!(m.new_monitor.hostname.is_none());
        assert!(m.new_monitor.port.is_none());
    }

    #[test]
    fn tcp_splits_host_and_port() {
        let json = r#"[
            {"name":"db","url":"tcp://db.example.com:5432"}
        ]"#;
        let plan = parse_and_map(json).unwrap();
        let m = &plan.mapped[0];
        assert!(m.new_monitor.url.is_none());
        assert_eq!(m.new_monitor.hostname.as_deref(), Some("db.example.com"));
        assert_eq!(m.new_monitor.port, Some(5432));
    }

    #[test]
    fn icmp_strips_scheme_into_hostname() {
        let json = r#"[
            {"name":"router","url":"icmp://10.0.0.1"}
        ]"#;
        let plan = parse_and_map(json).unwrap();
        let m = &plan.mapped[0];
        assert_eq!(m.new_monitor.hostname.as_deref(), Some("10.0.0.1"));
    }

    #[test]
    fn unknown_scheme_is_skipped() {
        let json = r#"[
            {"name":"weird","url":"gopher://example.com"}
        ]"#;
        let plan = parse_and_map(json).unwrap();
        assert!(plan.mapped.is_empty());
        assert_eq!(plan.skipped.len(), 1);
        assert_eq!(plan.skipped[0].source_kind, "gopher");
    }

    #[test]
    fn interval_defaults_to_60() {
        let json = r#"[
            {"name":"web","url":"https://example.com"}
        ]"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].new_monitor.interval_seconds, 60);
    }

    #[test]
    fn missing_name_is_skipped() {
        let json = r#"[
            {"url":"https://example.com"}
        ]"#;
        let plan = parse_and_map(json).unwrap();
        assert!(plan.mapped.is_empty());
        assert_eq!(plan.skipped.len(), 1);
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
