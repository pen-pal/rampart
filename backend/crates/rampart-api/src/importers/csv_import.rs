//! Generic CSV bulk-import format.
//!
//! Unlike the per-SaaS importers (each of which adapts one vendor's JSON
//! export shape), this importer reads a **Rampart-native CSV** — a flat
//! table an operator can hand-author in a spreadsheet, or generate from
//! whatever inventory system they already run, when no dedicated
//! importer exists. It is the lowest-common-denominator on-ramp.
//!
//! Because the input is CSV rather than JSON, the CLI dispatch in
//! `src/bin/import.rs` calls [`parse_csv_and_map`] (not the
//! `parse_and_map` entry point the JSON importers share) and hands it the
//! raw file text.
//!
//! ## Columns
//!
//! A header row is **required**. Recognised columns:
//!
//! ```text
//! name,kind,url,hostname,port,interval_seconds,timeout_seconds
//! ```
//!
//! - `name` — required for every row.
//! - `kind` — a Rampart [`MonitorKind`] string in `snake_case` (`http`,
//!   `tcp`, `ping`, `dns`, …). Rows with an unknown kind are skipped with
//!   a `tracing::warn!`.
//! - `url` / `hostname` / `port` — optional *per kind*; see the
//!   required-field table below. A row missing a field its kind requires
//!   is skipped.
//! - `interval_seconds` / `timeout_seconds` — optional; default to `60` /
//!   `16` when blank or unparseable.
//!
//! ## Required fields per kind
//!
//! | kind family                   | required beyond `name` + `kind` |
//! | ----------------------------- | ------------------------------- |
//! | `http`, `keyword`, `json_query`, `tls`, `grpc`, `websocket`, `doh`, `rdap`, `domain`, `browser` | `url`            |
//! | `tcp`                         | `hostname` + `port`             |
//! | everything else (network/service/db/banner) | `hostname`        |

use serde::Deserialize;
use serde_json::Value;
use tracing::warn;

use rampart_core::monitor::NewMonitor;
use rampart_core::MonitorKind;

pub use super::ImportError;
use super::{ImportPlan, MappedMonitor, SkippedMonitor};

/// One CSV row, deserialised by the `csv` crate against the header row.
/// Every field is optional at the parse layer (`csv` + `serde` map a
/// blank cell to `None`); per-kind required-field enforcement happens in
/// [`map_one`] so a single missing cell skips just that row rather than
/// failing the whole file.
#[derive(Debug, Deserialize)]
struct Row {
    name: Option<String>,
    kind: Option<String>,
    url: Option<String>,
    hostname: Option<String>,
    port: Option<String>,
    interval_seconds: Option<String>,
    timeout_seconds: Option<String>,
}

/// Parse a Rampart-native CSV inventory and map every well-formed row
/// onto a `NewMonitor`. Returns the mapped list + a list of skipped rows
/// (with reasons). Pure function — no I/O, no DB; the integration test
/// uses this directly without standing up Postgres.
///
/// A header row is required. Rows with an unknown `kind` or missing a
/// field their kind requires are skipped (with a `warn!`), not fatal.
pub fn parse_csv_and_map(csv: &str) -> Result<ImportPlan, ImportError> {
    // `flexible(true)` tolerates trailing empty trailing columns / ragged
    // rows the way a hand-edited spreadsheet export tends to produce.
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(csv.as_bytes());

    let mut rows: Vec<Row> = Vec::new();
    for result in reader.deserialize() {
        let row: Row = result.map_err(|e| ImportError::Csv(e.to_string()))?;
        rows.push(row);
    }

    if rows.is_empty() {
        return Err(ImportError::NoMonitors);
    }

    let mut plan = ImportPlan::default();
    for row in rows {
        match map_one(&row) {
            Ok(m) => plan.mapped.push(m),
            Err(s) => {
                warn!(
                    source_name = %s.source_name,
                    source_kind = %s.source_kind,
                    reason = %s.reason,
                    "skip: unmappable csv row",
                );
                plan.skipped.push(s);
            }
        }
    }
    Ok(plan)
}

/// Map a single CSV row onto a Rampart `NewMonitor`. Returns the mapped
/// form or a `SkippedMonitor` describing why we couldn't translate it.
fn map_one(row: &Row) -> Result<MappedMonitor, SkippedMonitor> {
    let kind_str = non_empty(&row.kind).unwrap_or_else(|| "<missing>".to_string());
    let source_kind = kind_str.clone();
    let source_name = non_empty(&row.name).unwrap_or_else(|| "<unnamed>".to_string());

    if source_name == "<unnamed>" {
        return Err(SkippedMonitor {
            source_name,
            source_kind,
            reason: "missing name".into(),
        });
    }

    let mapped_kind = match parse_kind(&kind_str) {
        Some(k) => k,
        None => {
            return Err(SkippedMonitor {
                source_name,
                source_kind,
                reason: format!("unknown kind `{kind_str}`"),
            });
        }
    };

    let url = non_empty(&row.url);
    let hostname = non_empty(&row.hostname);
    let port = non_empty(&row.port).and_then(|p| p.parse::<i32>().ok());

    // Per-kind required-field enforcement. `http`-family kinds probe a
    // full URL; `tcp` needs both host + port; every other kind probes a
    // bare host.
    match required(mapped_kind) {
        Required::Url => {
            if url.is_none() {
                return Err(missing(&source_name, &source_kind, "url"));
            }
        }
        Required::Hostname => {
            if hostname.is_none() {
                return Err(missing(&source_name, &source_kind, "hostname"));
            }
        }
        Required::HostnamePort => {
            if hostname.is_none() {
                return Err(missing(&source_name, &source_kind, "hostname"));
            }
            if port.is_none() {
                return Err(missing(&source_name, &source_kind, "port"));
            }
        }
    }

    let interval_seconds = non_empty(&row.interval_seconds)
        .and_then(|s| s.parse::<i32>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(60);
    let timeout_seconds = non_empty(&row.timeout_seconds)
        .and_then(|s| s.parse::<i32>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(16);

    let new_monitor = NewMonitor {
        name: source_name.clone(),
        kind: mapped_kind,
        url,
        hostname,
        port,
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
        agent_id: None,
    };

    Ok(MappedMonitor {
        source_name,
        source_kind,
        mapped_kind,
        new_monitor,
    })
}

/// Which extra field(s) a kind needs beyond `name` + `kind`.
enum Required {
    Url,
    Hostname,
    HostnamePort,
}

/// Pick the required-field set for a kind. `http`-family probes target a
/// full URL; `tcp` needs an explicit host + port; everything else probes
/// a bare host.
fn required(kind: MonitorKind) -> Required {
    use MonitorKind::*;
    match kind {
        Http | Keyword | JsonQuery | Tls | Grpc | Websocket | Doh | Rdap | Domain | Browser => {
            Required::Url
        }
        Tcp => Required::HostnamePort,
        _ => Required::Hostname,
    }
}

/// Build a "missing required field" skip record.
fn missing(name: &str, kind: &str, field: &str) -> SkippedMonitor {
    SkippedMonitor {
        source_name: name.to_string(),
        source_kind: kind.to_string(),
        reason: format!("missing required field `{field}` for kind `{kind}`"),
    }
}

/// Parse a Rampart `MonitorKind` from its `snake_case` string. Reuses the
/// canonical serde mapping on `MonitorKind` (the same one Postgres + the
/// API use) so the CSV column accepts exactly the kinds Rampart knows,
/// no hand-maintained second table to drift. Matched case-insensitively
/// for spreadsheet ergonomics.
fn parse_kind(s: &str) -> Option<MonitorKind> {
    let normalized = s.trim().to_ascii_lowercase();
    serde_json::from_value(Value::String(normalized)).ok()
}

/// Trim a cell and return `None` when it is absent or blank.
fn non_empty(field: &Option<String>) -> Option<String> {
    field
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_header_and_maps_kinds() {
        let csv = "name,kind,url,hostname,port,interval_seconds,timeout_seconds\n\
                   web,http,https://example.com,,,60,16\n\
                   db,tcp,,db.example.com,5432,120,10\n\
                   router,ping,,10.0.0.1,,60,16\n\
                   resolver,dns,,example.com,,60,16\n";
        let plan = parse_csv_and_map(csv).unwrap();
        assert_eq!(plan.mapped.len(), 4);
        assert_eq!(plan.mapped[0].mapped_kind, MonitorKind::Http);
        assert_eq!(plan.mapped[1].mapped_kind, MonitorKind::Tcp);
        assert_eq!(plan.mapped[2].mapped_kind, MonitorKind::Ping);
        assert_eq!(plan.mapped[3].mapped_kind, MonitorKind::Dns);
    }

    #[test]
    fn unknown_kind_is_skipped() {
        let csv = "name,kind,url,hostname,port,interval_seconds,timeout_seconds\n\
                   weird,banana,https://x,,,60,16\n";
        let plan = parse_csv_and_map(csv).unwrap();
        assert!(plan.mapped.is_empty());
        assert_eq!(plan.skipped.len(), 1);
        assert_eq!(plan.skipped[0].source_kind, "banana");
    }

    #[test]
    fn missing_required_field_skips_row() {
        // http without a url
        let csv = "name,kind,url,hostname,port,interval_seconds,timeout_seconds\n\
                   web,http,,,,60,16\n";
        let plan = parse_csv_and_map(csv).unwrap();
        assert!(plan.mapped.is_empty());
        assert_eq!(plan.skipped.len(), 1);
        assert!(plan.skipped[0].reason.contains("url"));
    }

    #[test]
    fn tcp_requires_hostname_and_port() {
        let csv = "name,kind,url,hostname,port,interval_seconds,timeout_seconds\n\
                   db,tcp,,db.example.com,,60,16\n";
        let plan = parse_csv_and_map(csv).unwrap();
        assert_eq!(plan.skipped.len(), 1);
        assert!(plan.skipped[0].reason.contains("port"));
    }

    #[test]
    fn quoted_field_with_embedded_comma() {
        let csv = "name,kind,url,hostname,port,interval_seconds,timeout_seconds\n\
                   \"web, primary\",http,\"https://example.com/a,b\",,,60,16\n";
        let plan = parse_csv_and_map(csv).unwrap();
        assert_eq!(plan.mapped.len(), 1);
        assert_eq!(plan.mapped[0].new_monitor.name, "web, primary");
        assert_eq!(
            plan.mapped[0].new_monitor.url.as_deref(),
            Some("https://example.com/a,b")
        );
    }

    #[test]
    fn defaults_applied_when_intervals_blank() {
        let csv = "name,kind,url,hostname,port,interval_seconds,timeout_seconds\n\
                   web,http,https://example.com,,,,\n";
        let plan = parse_csv_and_map(csv).unwrap();
        let m = &plan.mapped[0];
        assert_eq!(m.new_monitor.interval_seconds, 60);
        assert_eq!(m.new_monitor.timeout_seconds, 16);
    }

    #[test]
    fn empty_file_errors() {
        let csv = "name,kind,url,hostname,port,interval_seconds,timeout_seconds\n";
        let err = parse_csv_and_map(csv).unwrap_err();
        assert!(matches!(err, ImportError::NoMonitors));
    }
}
