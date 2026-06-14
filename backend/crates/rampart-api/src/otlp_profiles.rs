//! Minimal OTLP profiles (`v1development`) decoder → folded-stack maps.
//!
//! The OpenTelemetry profiling signal is still `development`, and the maintained
//! `opentelemetry-proto` crate's generated types lag the published proto (they
//! model the older tables-in-`ProfilesData` shape, not the current
//! `ExportProfilesServiceRequest{resource_profiles, dictionary}` wire format).
//! So — as with pprof — we hand-write the handful of messages we need as `prost`
//! types, matching the current `v1development` proto, so we interop with real
//! exporters. `prost` skips unknown fields, so we declare only what we read.
//!
//! Wire model (the "dictionary"): shared `location_table` / `function_table` /
//! `string_table` live once in `dictionary`; a `Sample` names its locations via
//! `[locations_start_index, +locations_length)` into the profile's
//! `location_indices`, each entry an index into `location_table`; a location's
//! lines name functions via `function_index`; a function's name is a
//! `string_table` index. Leaf-first frames reverse to leaf-last (folded
//! convention). See `docs/design/PROFILING.md`.

use prost::Message;
use rampart_core::profile::FoldedMap;

#[derive(Clone, PartialEq, Message)]
pub struct ExportProfilesServiceRequest {
    #[prost(message, repeated, tag = "1")]
    pub resource_profiles: Vec<ResourceProfiles>,
    #[prost(message, optional, tag = "2")]
    pub dictionary: Option<ProfilesDictionary>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ProfilesDictionary {
    #[prost(message, repeated, tag = "2")]
    pub location_table: Vec<Location>,
    #[prost(message, repeated, tag = "3")]
    pub function_table: Vec<Function>,
    #[prost(string, repeated, tag = "5")]
    pub string_table: Vec<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ResourceProfiles {
    #[prost(message, optional, tag = "1")]
    pub resource: Option<Resource>,
    #[prost(message, repeated, tag = "2")]
    pub scope_profiles: Vec<ScopeProfiles>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ScopeProfiles {
    #[prost(message, repeated, tag = "2")]
    pub profiles: Vec<Profile>,
}

#[derive(Clone, PartialEq, Message)]
pub struct Profile {
    #[prost(message, repeated, tag = "1")]
    pub sample_type: Vec<ValueType>,
    #[prost(message, repeated, tag = "2")]
    pub sample: Vec<Sample>,
    #[prost(int32, repeated, tag = "3")]
    pub location_indices: Vec<i32>,
    #[prost(int64, tag = "5")]
    pub duration_nanos: i64,
    #[prost(int64, tag = "7")]
    pub period: i64,
    #[prost(int32, tag = "9")]
    pub default_sample_type_index: i32,
}

#[derive(Clone, PartialEq, Message)]
pub struct ValueType {
    #[prost(int32, tag = "1")]
    pub type_strindex: i32,
}

#[derive(Clone, PartialEq, Message)]
pub struct Sample {
    #[prost(int32, tag = "1")]
    pub locations_start_index: i32,
    #[prost(int32, tag = "2")]
    pub locations_length: i32,
    #[prost(int64, repeated, tag = "3")]
    pub value: Vec<i64>,
}

#[derive(Clone, PartialEq, Message)]
pub struct Location {
    #[prost(message, repeated, tag = "3")]
    pub line: Vec<Line>,
}

#[derive(Clone, PartialEq, Message)]
pub struct Line {
    #[prost(int32, tag = "1")]
    pub function_index: i32,
}

#[derive(Clone, PartialEq, Message)]
pub struct Function {
    #[prost(int32, tag = "1")]
    pub name_strindex: i32,
}

// Minimal slices of the common/resource protos — just enough to read
// `service.name` from a resource's attributes.
#[derive(Clone, PartialEq, Message)]
pub struct Resource {
    #[prost(message, repeated, tag = "1")]
    pub attributes: Vec<KeyValue>,
}

#[derive(Clone, PartialEq, Message)]
pub struct KeyValue {
    #[prost(string, tag = "1")]
    pub key: String,
    #[prost(message, optional, tag = "2")]
    pub value: Option<AnyValue>,
}

#[derive(Clone, PartialEq, Message)]
pub struct AnyValue {
    // AnyValue is a oneof; its string variant is field 1, which on the wire
    // encodes identically to an optional string here.
    #[prost(string, optional, tag = "1")]
    pub string_value: Option<String>,
}

/// One decoded profile lowered to a folded map + the fields we persist.
pub struct OtlpProfile {
    pub service: String,
    pub type_name: Option<String>,
    pub period_ns: i64,
    pub duration_ns: i64,
    pub folded: FoldedMap,
}

/// Decode an OTLP `ExportProfilesServiceRequest` into per-profile folded maps.
pub fn parse_otlp_profiles(bytes: &[u8]) -> Result<Vec<OtlpProfile>, String> {
    let req =
        ExportProfilesServiceRequest::decode(bytes).map_err(|e| format!("OTLP decode: {e}"))?;
    let dict = req
        .dictionary
        .ok_or_else(|| "OTLP profiles request has no dictionary".to_string())?;

    let mut out = Vec::new();
    for rp in &req.resource_profiles {
        let service = rp
            .resource
            .as_ref()
            .and_then(service_name)
            .unwrap_or_else(|| "unknown".to_string());
        for sp in &rp.scope_profiles {
            for p in &sp.profiles {
                out.push(fold_profile(p, &dict, &service));
            }
        }
    }
    Ok(out)
}

fn fold_profile(p: &Profile, dict: &ProfilesDictionary, service: &str) -> OtlpProfile {
    let st = &dict.string_table;
    let str_at = |i: i32| -> Option<&str> {
        usize::try_from(i)
            .ok()
            .and_then(|i| st.get(i))
            .map(String::as_str)
    };
    // value column = the default sample type (clamped to a valid index).
    let vi = if p.sample_type.is_empty() {
        0
    } else {
        (p.default_sample_type_index.max(0) as usize).min(p.sample_type.len() - 1)
    };
    let type_name = p
        .sample_type
        .get(vi)
        .and_then(|vt| str_at(vt.type_strindex))
        .map(str::to_string);

    let mut folded = FoldedMap::new();
    for s in &p.sample {
        let val = s.value.get(vi).copied().unwrap_or(0);
        if val <= 0 {
            continue;
        }
        let start = s.locations_start_index.max(0) as usize;
        let len = s.locations_length.max(0) as usize;
        let mut frames: Vec<&str> = Vec::new();
        for k in start..start.saturating_add(len) {
            let Some(&loc_idx) = p.location_indices.get(k) else {
                continue;
            };
            let Some(loc) = usize::try_from(loc_idx)
                .ok()
                .and_then(|i| dict.location_table.get(i))
            else {
                continue;
            };
            for line in &loc.line {
                if let Some(f) = usize::try_from(line.function_index)
                    .ok()
                    .and_then(|i| dict.function_table.get(i))
                {
                    if let Some(name) = str_at(f.name_strindex).filter(|n| !n.is_empty()) {
                        frames.push(name);
                    }
                }
            }
        }
        if frames.is_empty() {
            continue;
        }
        frames.reverse(); // leaf-first → leaf-last
        *folded.entry(frames.join(";")).or_insert(0) += val;
    }

    OtlpProfile {
        service: service.to_string(),
        type_name,
        period_ns: p.period,
        duration_ns: p.duration_nanos,
        folded,
    }
}

fn service_name(r: &Resource) -> Option<String> {
    r.attributes
        .iter()
        .find(|kv| kv.key == "service.name")
        .and_then(|kv| kv.value.as_ref())
        .and_then(|v| v.string_value.clone())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_otlp_dictionary_model() {
        // string_table[0]="" per spec; then names.
        let dict = ProfilesDictionary {
            string_table: vec!["".into(), "cpu".into(), "main".into(), "work".into()],
            function_table: vec![
                Function { name_strindex: 2 }, // main
                Function { name_strindex: 3 }, // work
            ],
            location_table: vec![
                Location {
                    line: vec![Line { function_index: 0 }],
                }, // main
                Location {
                    line: vec![Line { function_index: 1 }],
                }, // work
            ],
        };
        let profile = Profile {
            sample_type: vec![ValueType { type_strindex: 1 }], // "cpu"
            // location_indices: [0]=main, [1]=work. Sample below uses [1,0] order
            // via start=0,len=2 -> leaf-first work,main? We lay out indices so the
            // slice is [work, main] (leaf-first).
            location_indices: vec![1, 0],
            sample: vec![Sample {
                locations_start_index: 0,
                locations_length: 2,
                value: vec![9],
            }],
            duration_nanos: 2000,
            period: 100,
            default_sample_type_index: 0,
        };
        let req = ExportProfilesServiceRequest {
            resource_profiles: vec![ResourceProfiles {
                resource: Some(Resource {
                    attributes: vec![KeyValue {
                        key: "service.name".into(),
                        value: Some(AnyValue {
                            string_value: Some("api".into()),
                        }),
                    }],
                }),
                scope_profiles: vec![ScopeProfiles {
                    profiles: vec![profile],
                }],
            }],
            dictionary: Some(dict),
        };
        let mut buf = Vec::new();
        req.encode(&mut buf).unwrap();

        let parsed = parse_otlp_profiles(&buf).unwrap();
        assert_eq!(parsed.len(), 1);
        let p = &parsed[0];
        assert_eq!(p.service, "api");
        assert_eq!(p.type_name.as_deref(), Some("cpu"));
        assert_eq!(p.period_ns, 100);
        // leaf-first [work, main] reversed → "main;work"
        assert_eq!(p.folded.get("main;work"), Some(&9));
    }
}
