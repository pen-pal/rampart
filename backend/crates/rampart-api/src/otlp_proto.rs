//! OTLP/protobuf decode for traces + logs → the same `ParsedSpan` / `ParsedLog`
//! the JSON paths produce. Uses the `opentelemetry-proto` generated messages.
//! OTLP ids are bytes here, so they're hex-encoded to match the JSON form.

use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::common::v1::{any_value, AnyValue, KeyValue};
use prost::Message;
use rampart_core::log::ParsedLog;
use rampart_core::trace::ParsedSpan;

pub fn parse_otlp_traces_protobuf(bytes: &[u8]) -> Result<Vec<ParsedSpan>, prost::DecodeError> {
    let req = ExportTraceServiceRequest::decode(bytes)?;
    let mut out = Vec::new();
    for rs in req.resource_spans {
        let service_name = rs
            .resource
            .as_ref()
            .and_then(|r| attr_string(&r.attributes, "service.name"))
            .unwrap_or_else(|| "unknown".to_string());
        for ss in rs.scope_spans {
            for sp in ss.spans {
                if sp.trace_id.is_empty() || sp.span_id.is_empty() {
                    continue;
                }
                let (status_code, status_message) = match sp.status {
                    Some(st) => (
                        // OTLP status code is 0=unset / 1=ok / 2=error; clamp an
                        // out-of-spec value rather than truncate it via `as`.
                        st.code.clamp(0, 2) as i16,
                        if st.message.is_empty() {
                            None
                        } else {
                            Some(st.message)
                        },
                    ),
                    None => (0, None),
                };
                out.push(ParsedSpan {
                    trace_id: hex::encode(&sp.trace_id),
                    span_id: hex::encode(&sp.span_id),
                    parent_span_id: if sp.parent_span_id.is_empty() {
                        None
                    } else {
                        Some(hex::encode(&sp.parent_span_id))
                    },
                    service_name: service_name.clone(),
                    name: sp.name,
                    // Span kind is 0..=5; clamp out-of-spec values.
                    kind: sp.kind.clamp(0, 5) as i16,
                    // u64 nanos: a value past i64::MAX would wrap negative under
                    // `as i64`, corrupting duration/ordering. Fall back to 0.
                    start_ns: i64::try_from(sp.start_time_unix_nano).unwrap_or(0),
                    end_ns: i64::try_from(sp.end_time_unix_nano).unwrap_or(0),
                    status_code,
                    status_message,
                    attributes: attrs_to_object(&sp.attributes),
                });
            }
        }
    }
    Ok(out)
}

fn attr_string(attrs: &[KeyValue], key: &str) -> Option<String> {
    attrs
        .iter()
        .find(|kv| kv.key == key)
        .and_then(|kv| kv.value.as_ref())
        .and_then(|v| match v.value.as_ref() {
            Some(any_value::Value::StringValue(s)) => Some(s.clone()),
            _ => None,
        })
}

fn attrs_to_object(attrs: &[KeyValue]) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for kv in attrs {
        if let Some(v) = kv.value.as_ref().and_then(any_to_json) {
            map.insert(kv.key.clone(), v);
        }
    }
    serde_json::Value::Object(map)
}

fn any_to_json(v: &AnyValue) -> Option<serde_json::Value> {
    match v.value.as_ref()? {
        any_value::Value::StringValue(s) => Some(serde_json::Value::String(s.clone())),
        any_value::Value::BoolValue(b) => Some(serde_json::Value::Bool(*b)),
        any_value::Value::IntValue(i) => Some(serde_json::Value::from(*i)),
        any_value::Value::DoubleValue(d) => Some(serde_json::Value::from(*d)),
        _ => None,
    }
}

/// Decode an OTLP/protobuf `ExportLogsServiceRequest` into [`ParsedLog`]s
/// (lowered to the same shape as the JSON path in `rampart_core::log`).
pub fn parse_otlp_logs_protobuf(bytes: &[u8]) -> Result<Vec<ParsedLog>, prost::DecodeError> {
    let req = ExportLogsServiceRequest::decode(bytes)?;
    let mut out = Vec::new();
    for rl in req.resource_logs {
        let service_name = rl
            .resource
            .as_ref()
            .and_then(|r| attr_string(&r.attributes, "service.name"))
            .unwrap_or_else(|| "unknown".to_string());
        for sl in rl.scope_logs {
            for lr in sl.log_records {
                // u64 nanos → i64, overflow-safe: prefer time_unix_nano when it
                // is set and fits, else observed_time_unix_nano, else 0 (a
                // wrapping `as i64` would land negative and mis-stamp the log).
                let to_i64 = |n: u64| i64::try_from(n).ok();
                let time_ns = to_i64(lr.time_unix_nano)
                    .filter(|&n| n > 0)
                    .or_else(|| to_i64(lr.observed_time_unix_nano))
                    .unwrap_or(0);
                out.push(ParsedLog {
                    time_ns,
                    // OTLP severity number is 1..=24 (0 = unspecified); clamp.
                    severity: lr.severity_number.clamp(0, 24) as i16,
                    severity_text: if lr.severity_text.is_empty() {
                        None
                    } else {
                        Some(lr.severity_text)
                    },
                    service_name: service_name.clone(),
                    body: lr.body.as_ref().map(any_value_string).unwrap_or_default(),
                    trace_id: hex_id(&lr.trace_id),
                    span_id: hex_id(&lr.span_id),
                    attributes: attrs_to_object(&lr.attributes),
                });
            }
        }
    }
    Ok(out)
}

/// A log body's string form (StringValue, else a compact rendering).
fn any_value_string(v: &AnyValue) -> String {
    match v.value.as_ref() {
        Some(any_value::Value::StringValue(s)) => s.clone(),
        Some(other) => format!("{other:?}"),
        None => String::new(),
    }
}

fn hex_id(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        None
    } else {
        Some(hex::encode(bytes))
    }
}
