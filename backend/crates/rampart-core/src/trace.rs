//! Distributed tracing — OTLP span model + ingest parsing.
//!
//! Rampart ingests OpenTelemetry spans (OTLP). A *trace* is the set of spans
//! sharing a `trace_id`, assembled on read. [`parse_otlp_traces_json`] turns an
//! OTLP/HTTP JSON `ExportTraceServiceRequest` into [`ParsedSpan`]s; it's pure
//! and unit-tested. (The protobuf encoding is decoded in the API layer and
//! lowered to the same `ParsedSpan`.) See `docs/design/TRACES.md`.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// A span ready to store (post-parse, pre-insert).
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedSpan {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub service_name: String,
    pub name: String,
    pub kind: i16,
    pub start_ns: i64,
    pub end_ns: i64,
    pub status_code: i16,
    pub status_message: Option<String>,
    pub attributes: serde_json::Value,
}

impl ParsedSpan {
    /// Wall-clock duration in milliseconds (non-negative; clamps if the
    /// exporter sent end < start).
    pub fn duration_ms(&self) -> f64 {
        ((self.end_ns - self.start_ns).max(0)) as f64 / 1_000_000.0
    }
}

/// One stored span (read side — trace detail / waterfall).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Span {
    pub span_id: String,
    pub trace_id: String,
    pub parent_span_id: Option<String>,
    pub service_name: String,
    pub name: String,
    pub kind: i16,
    pub start_ns: i64,
    pub end_ns: i64,
    pub duration_ms: f64,
    pub status_code: i16,
    pub status_message: Option<String>,
    pub attributes: Option<serde_json::Value>,
    pub received_at: OffsetDateTime,
}

/// Trace list row (one per trace_id) — built by an aggregate query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceSummary {
    pub trace_id: String,
    /// Root span's service + operation (the entry point of the trace).
    pub root_service: String,
    pub root_name: String,
    pub start_ns: i64,
    pub duration_ms: f64,
    pub span_count: i64,
    pub error_count: i64,
    pub services: Vec<String>,
    pub started_at: OffsetDateTime,
}

/// One directed edge in the service dependency map (caller → callee), with a
/// call count, derived from cross-service parent/child span pairs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceEdge {
    pub from_service: String,
    pub to_service: String,
    pub calls: i64,
    /// Callee spans on this edge with an error status.
    pub errors: i64,
    /// p95 latency (ms) of the callee spans on this edge, if computable.
    pub p95_ms: Option<f64>,
}

/// Per-(service, operation) APM rollup over a time window — the "services /
/// resources" numbers (latency percentiles, throughput, error rate).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationStat {
    pub service: String,
    pub operation: String,
    pub calls: i64,
    pub errors: i64,
    /// Error rate as a percentage (0–100).
    pub error_rate: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub avg_ms: f64,
    pub max_ms: f64,
}

// ─────────────────────────── OTLP/JSON parsing ───────────────────────────

/// Parse an OTLP/HTTP JSON `ExportTraceServiceRequest` into spans. Tolerant:
/// malformed or missing pieces are skipped rather than failing the batch, so
/// one bad span doesn't drop the whole export.
pub fn parse_otlp_traces_json(v: &serde_json::Value) -> Vec<ParsedSpan> {
    let mut out = Vec::new();
    let Some(resource_spans) = v.get("resourceSpans").and_then(|x| x.as_array()) else {
        return out;
    };
    for rs in resource_spans {
        let resource_attrs = rs.get("resource").and_then(|r| r.get("attributes"));
        let service_name =
            attr_str(resource_attrs, "service.name").unwrap_or_else(|| "unknown".to_string());

        for ss in array(rs.get("scopeSpans")) {
            for sp in array(ss.get("spans")) {
                let (Some(trace_id), Some(span_id)) = (
                    sp.get("traceId").and_then(|x| x.as_str()),
                    sp.get("spanId").and_then(|x| x.as_str()),
                ) else {
                    continue;
                };
                if trace_id.is_empty() || span_id.is_empty() {
                    continue;
                }
                let parent = sp
                    .get("parentSpanId")
                    .and_then(|x| x.as_str())
                    .filter(|s| !s.is_empty())
                    .map(String::from);
                let (status_code, status_message) = parse_status(sp.get("status"));

                out.push(ParsedSpan {
                    trace_id: trace_id.to_string(),
                    span_id: span_id.to_string(),
                    parent_span_id: parent,
                    service_name: service_name.clone(),
                    name: sp
                        .get("name")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string(),
                    kind: sp.get("kind").and_then(|x| x.as_i64()).unwrap_or(0) as i16,
                    start_ns: u64_field(sp.get("startTimeUnixNano")),
                    end_ns: u64_field(sp.get("endTimeUnixNano")),
                    status_code,
                    status_message,
                    attributes: attrs_to_object(sp.get("attributes")),
                });
            }
        }
    }
    out
}

pub(crate) fn array(v: Option<&serde_json::Value>) -> impl Iterator<Item = &serde_json::Value> {
    v.and_then(|x| x.as_array()).into_iter().flatten()
}

/// Read an OTLP attribute list (`[{key, value:{stringValue|intValue|…}}]`) for
/// a single string-valued key.
pub(crate) fn attr_str(attrs: Option<&serde_json::Value>, key: &str) -> Option<String> {
    for a in array(attrs) {
        if a.get("key").and_then(|k| k.as_str()) == Some(key) {
            return a
                .get("value")
                .and_then(|val| val.get("stringValue"))
                .and_then(|s| s.as_str())
                .map(String::from);
        }
    }
    None
}

/// Flatten an OTLP attribute list to a plain JSON object of scalar values.
pub(crate) fn attrs_to_object(attrs: Option<&serde_json::Value>) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for a in array(attrs) {
        let Some(key) = a.get("key").and_then(|k| k.as_str()) else {
            continue;
        };
        if let Some(val) = a.get("value").map(otlp_any_value) {
            map.insert(key.to_string(), val);
        }
    }
    serde_json::Value::Object(map)
}

/// Lower an OTLP `AnyValue` to a plain JSON scalar (best-effort).
pub(crate) fn otlp_any_value(v: &serde_json::Value) -> serde_json::Value {
    if let Some(s) = v.get("stringValue").and_then(|x| x.as_str()) {
        return serde_json::Value::String(s.to_string());
    }
    if let Some(b) = v.get("boolValue").and_then(|x| x.as_bool()) {
        return serde_json::Value::Bool(b);
    }
    // intValue arrives as a string per OTLP/JSON; doubleValue as a number.
    if let Some(i) = v.get("intValue") {
        if let Some(n) = i.as_i64() {
            return serde_json::Value::from(n);
        }
        if let Some(s) = i.as_str().and_then(|s| s.parse::<i64>().ok()) {
            return serde_json::Value::from(s);
        }
    }
    if let Some(d) = v.get("doubleValue").and_then(|x| x.as_f64()) {
        return serde_json::Value::from(d);
    }
    serde_json::Value::Null
}

/// OTLP unsigned 64-bit fields are JSON strings (or sometimes numbers).
pub(crate) fn u64_field(v: Option<&serde_json::Value>) -> i64 {
    match v {
        Some(serde_json::Value::String(s)) => s.parse::<i64>().unwrap_or(0),
        Some(serde_json::Value::Number(n)) => n.as_i64().unwrap_or(0),
        _ => 0,
    }
}

fn parse_status(v: Option<&serde_json::Value>) -> (i16, Option<String>) {
    let Some(st) = v else { return (0, None) };
    let code = st.get("code").and_then(|c| c.as_i64()).unwrap_or(0) as i16;
    let msg = st
        .get("message")
        .and_then(|m| m.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);
    (code, msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_otlp_json_batch() {
        let payload = json!({
            "resourceSpans": [{
                "resource": { "attributes": [
                    { "key": "service.name", "value": { "stringValue": "checkout" } }
                ] },
                "scopeSpans": [{
                    "spans": [
                        {
                            "traceId": "abc123", "spanId": "root1",
                            "name": "POST /checkout", "kind": 2,
                            "startTimeUnixNano": "1700000000000000000",
                            "endTimeUnixNano":   "1700000000050000000",
                            "status": { "code": 1 },
                            "attributes": [
                                { "key": "http.method", "value": { "stringValue": "POST" } },
                                { "key": "http.status_code", "value": { "intValue": "200" } }
                            ]
                        },
                        {
                            "traceId": "abc123", "spanId": "child1", "parentSpanId": "root1",
                            "name": "SELECT carts", "kind": 3,
                            "startTimeUnixNano": "1700000000010000000",
                            "endTimeUnixNano":   "1700000000020000000",
                            "status": { "code": 2, "message": "deadlock" }
                        }
                    ]
                }]
            }]
        });
        let spans = parse_otlp_traces_json(&payload);
        assert_eq!(spans.len(), 2);

        let root = &spans[0];
        assert_eq!(root.trace_id, "abc123");
        assert_eq!(root.span_id, "root1");
        assert_eq!(root.parent_span_id, None);
        assert_eq!(root.service_name, "checkout");
        assert_eq!(root.name, "POST /checkout");
        assert_eq!(root.kind, 2);
        assert_eq!(root.status_code, 1);
        assert!((root.duration_ms() - 50.0).abs() < 0.001);
        assert_eq!(root.attributes.get("http.method").unwrap(), "POST");
        assert_eq!(root.attributes.get("http.status_code").unwrap(), 200);

        let child = &spans[1];
        assert_eq!(child.parent_span_id.as_deref(), Some("root1"));
        assert_eq!(child.status_code, 2);
        assert_eq!(child.status_message.as_deref(), Some("deadlock"));
    }

    #[test]
    fn tolerates_missing_and_empty() {
        assert!(parse_otlp_traces_json(&json!({})).is_empty());
        assert!(parse_otlp_traces_json(&json!({ "resourceSpans": [] })).is_empty());
        // Span missing required ids is skipped, not fatal.
        let p = json!({ "resourceSpans": [{ "scopeSpans": [{ "spans": [
            { "name": "no ids" },
            { "traceId": "t", "spanId": "s", "name": "ok" }
        ] }] }] });
        let spans = parse_otlp_traces_json(&p);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].service_name, "unknown"); // no resource service.name
    }

    #[test]
    fn duration_clamps_when_end_before_start() {
        let s = ParsedSpan {
            trace_id: "t".into(),
            span_id: "s".into(),
            parent_span_id: None,
            service_name: "x".into(),
            name: "n".into(),
            kind: 0,
            start_ns: 100,
            end_ns: 50,
            status_code: 0,
            status_message: None,
            attributes: serde_json::Value::Null,
        };
        assert_eq!(s.duration_ms(), 0.0);
    }
}
