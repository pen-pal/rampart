//! Log ingestion — OTLP log model + parsing.
//!
//! Rampart ingests OpenTelemetry logs (OTLP). [`parse_otlp_logs_json`] turns an
//! OTLP/HTTP JSON `ExportLogsServiceRequest` into [`ParsedLog`]s (reusing the
//! shared OTLP attribute helpers in [`crate::trace`]); the protobuf encoding is
//! decoded in the API layer and lowered to the same `ParsedLog`. Logs carry an
//! optional `trace_id`/`span_id` so they correlate with the traces tier. See
//! `docs/design/LOGS.md`.

use crate::trace::{attr_str, attrs_to_object, u64_field};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// A log record ready to store (post-parse, pre-insert).
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedLog {
    /// Event time in unix nanos (0 when the exporter sent neither time).
    pub time_ns: i64,
    pub severity: i16,
    pub severity_text: Option<String>,
    pub service_name: String,
    pub body: String,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub attributes: serde_json::Value,
}

/// One stored log line (read side).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub id: uuid::Uuid,
    pub ts: OffsetDateTime,
    pub severity: i16,
    pub severity_text: Option<String>,
    /// Coarse level derived from the severity number (see [`coarse_level`]).
    pub level: String,
    pub service_name: String,
    pub body: String,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub attributes: Option<serde_json::Value>,
}

/// Map an OTLP severity number (1–24) to a coarse level name used for
/// filtering + colour. 0/unknown → "info".
pub fn coarse_level(severity: i16) -> &'static str {
    match severity {
        1..=4 => "trace",
        5..=8 => "debug",
        9..=12 => "info",
        13..=16 => "warn",
        17..=20 => "error",
        21..=24 => "fatal",
        _ => "info",
    }
}

/// Lowest OTLP severity number for a coarse level — used to translate a
/// "minimum level" filter into a numeric threshold.
pub fn level_min_severity(level: &str) -> Option<i16> {
    match level {
        "trace" => Some(1),
        "debug" => Some(5),
        "info" => Some(9),
        "warn" => Some(13),
        "error" => Some(17),
        "fatal" => Some(21),
        _ => None,
    }
}

/// Parse an OTLP/HTTP JSON `ExportLogsServiceRequest` into log records.
/// Tolerant: malformed pieces are skipped, not fatal.
pub fn parse_otlp_logs_json(v: &serde_json::Value) -> Vec<ParsedLog> {
    let mut out = Vec::new();
    let Some(resource_logs) = v.get("resourceLogs").and_then(|x| x.as_array()) else {
        return out;
    };
    for rl in resource_logs {
        let resource_attrs = rl.get("resource").and_then(|r| r.get("attributes"));
        let service_name =
            attr_str(resource_attrs, "service.name").unwrap_or_else(|| "unknown".to_string());

        for sl in rl
            .get("scopeLogs")
            .and_then(|x| x.as_array())
            .into_iter()
            .flatten()
        {
            for lr in sl
                .get("logRecords")
                .and_then(|x| x.as_array())
                .into_iter()
                .flatten()
            {
                // Prefer event time; fall back to observed time.
                let time_ns = {
                    let t = u64_field(lr.get("timeUnixNano"));
                    if t > 0 {
                        t
                    } else {
                        u64_field(lr.get("observedTimeUnixNano"))
                    }
                };
                out.push(ParsedLog {
                    time_ns,
                    severity: lr
                        .get("severityNumber")
                        .and_then(|x| x.as_i64())
                        .unwrap_or(0) as i16,
                    severity_text: lr
                        .get("severityText")
                        .and_then(|x| x.as_str())
                        .filter(|s| !s.is_empty())
                        .map(String::from),
                    service_name: service_name.clone(),
                    body: body_string(lr.get("body")),
                    trace_id: hexish(lr.get("traceId")),
                    span_id: hexish(lr.get("spanId")),
                    attributes: attrs_to_object(lr.get("attributes")),
                });
            }
        }
    }
    out
}

/// OTLP log `body` is an AnyValue — usually `{stringValue}`. Extract the string
/// form; non-string bodies are rendered compactly.
fn body_string(v: Option<&serde_json::Value>) -> String {
    match v {
        Some(b) => {
            if let Some(s) = b.get("stringValue").and_then(|x| x.as_str()) {
                s.to_string()
            } else if b.is_null() {
                String::new()
            } else {
                b.to_string()
            }
        }
        None => String::new(),
    }
}

/// A non-empty hex id string, or None.
fn hexish(v: Option<&serde_json::Value>) -> Option<String> {
    v.and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_otlp_logs_batch() {
        let payload = json!({
            "resourceLogs": [{
                "resource": { "attributes": [
                    { "key": "service.name", "value": { "stringValue": "api" } }
                ] },
                "scopeLogs": [{
                    "logRecords": [
                        {
                            "timeUnixNano": "1700000000000000000",
                            "severityNumber": 17, "severityText": "ERROR",
                            "body": { "stringValue": "db connection failed" },
                            "traceId": "abc123", "spanId": "def456",
                            "attributes": [
                                { "key": "retry", "value": { "intValue": "3" } }
                            ]
                        },
                        {
                            "observedTimeUnixNano": "1700000000500000000",
                            "severityNumber": 9,
                            "body": { "stringValue": "request handled" }
                        }
                    ]
                }]
            }]
        });
        let logs = parse_otlp_logs_json(&payload);
        assert_eq!(logs.len(), 2);

        let err = &logs[0];
        assert_eq!(err.service_name, "api");
        assert_eq!(err.severity, 17);
        assert_eq!(err.severity_text.as_deref(), Some("ERROR"));
        assert_eq!(err.body, "db connection failed");
        assert_eq!(err.trace_id.as_deref(), Some("abc123"));
        assert_eq!(err.span_id.as_deref(), Some("def456"));
        assert_eq!(err.attributes.get("retry").unwrap(), 3);
        assert_eq!(coarse_level(err.severity), "error");

        let info = &logs[1];
        assert_eq!(info.time_ns, 1_700_000_000_500_000_000); // observed-time fallback
        assert_eq!(info.trace_id, None);
        assert_eq!(coarse_level(info.severity), "info");
    }

    #[test]
    fn tolerates_empty() {
        assert!(parse_otlp_logs_json(&json!({})).is_empty());
        assert!(parse_otlp_logs_json(&json!({ "resourceLogs": [] })).is_empty());
    }

    #[test]
    fn level_mapping_round_trips() {
        assert_eq!(coarse_level(0), "info");
        assert_eq!(coarse_level(13), "warn");
        assert_eq!(coarse_level(24), "fatal");
        assert_eq!(level_min_severity("warn"), Some(13));
        assert_eq!(level_min_severity("nonsense"), None);
    }
}
