//! OTLP/protobuf trace decode → [`ParsedSpan`].
//!
//! Decodes an `ExportTraceServiceRequest` protobuf (the default OTLP/HTTP
//! encoding) using the `opentelemetry-proto` generated messages, then lowers
//! it to the same `ParsedSpan` the JSON path produces (`rampart_core::trace`).
//! OTLP ids are bytes here, so they're hex-encoded to match the JSON form.

use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::common::v1::{any_value, AnyValue, KeyValue};
use prost::Message;
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
                        st.code as i16,
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
                    kind: sp.kind as i16,
                    start_ns: sp.start_time_unix_nano as i64,
                    end_ns: sp.end_time_unix_nano as i64,
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
