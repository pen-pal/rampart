//! Minimal pprof (`profile.proto`) decoder — just enough to lower a CPU/alloc
//! profile into a folded-stack map.
//!
//! pprof is Google's de-facto profile format: a gzipped protobuf emitted by Go
//! `runtime/pprof`, the Rust `pprof` crate, py-spy, async-profiler, the
//! Pyroscope/Parca agents, and more. Rather than pull in the heavy `pprof`
//! profiler crate (it samples the *current* process), we hand-write the handful
//! of `profile.proto` messages we need as `prost` types and decode directly.
//!
//! Folded conversion: a pprof `Sample` carries `location_id`s **leaf-first**;
//! each location resolves to one or more `Line`s (inlined frames), each naming a
//! `Function` whose name is a `string_table` index. We collect the frame names,
//! reverse to leaf-last (root-first, the folded convention), join with `;`, and
//! sum the chosen value column. See `docs/design/PROFILING.md`.

use prost::Message;
use rampart_core::profile::FoldedMap;
use std::collections::HashMap;

#[derive(Clone, PartialEq, Message)]
pub struct Profile {
    #[prost(message, repeated, tag = "1")]
    pub sample_type: Vec<ValueType>,
    #[prost(message, repeated, tag = "2")]
    pub sample: Vec<Sample>,
    #[prost(message, repeated, tag = "4")]
    pub location: Vec<Location>,
    #[prost(message, repeated, tag = "5")]
    pub function: Vec<Function>,
    #[prost(string, repeated, tag = "6")]
    pub string_table: Vec<String>,
    #[prost(int64, tag = "10")]
    pub duration_nanos: i64,
    #[prost(message, optional, tag = "11")]
    pub period_type: Option<ValueType>,
    #[prost(int64, tag = "12")]
    pub period: i64,
}

#[derive(Clone, PartialEq, Message)]
pub struct ValueType {
    #[prost(int64, tag = "1")]
    pub r#type: i64,
    #[prost(int64, tag = "2")]
    pub unit: i64,
}

#[derive(Clone, PartialEq, Message)]
pub struct Sample {
    #[prost(uint64, repeated, tag = "1")]
    pub location_id: Vec<u64>,
    #[prost(int64, repeated, tag = "2")]
    pub value: Vec<i64>,
}

#[derive(Clone, PartialEq, Message)]
pub struct Location {
    #[prost(uint64, tag = "1")]
    pub id: u64,
    #[prost(message, repeated, tag = "4")]
    pub line: Vec<Line>,
}

#[derive(Clone, PartialEq, Message)]
pub struct Line {
    #[prost(uint64, tag = "1")]
    pub function_id: u64,
}

#[derive(Clone, PartialEq, Message)]
pub struct Function {
    #[prost(uint64, tag = "1")]
    pub id: u64,
    #[prost(int64, tag = "2")]
    pub name: i64,
}

/// A decoded pprof lowered to a folded map plus the metadata we persist.
pub struct Parsed {
    pub folded: FoldedMap,
    /// Profile type name (from the chosen sample-type's string), e.g. "cpu".
    pub type_name: Option<String>,
    pub period_ns: i64,
    pub duration_ns: i64,
}

/// Decode a (optionally gzipped) pprof body into a folded map. `value_index`
/// picks which sample value column to sum; `None` defaults to the last column
/// (for `[count, nanos]` profiles that's the nanos/bytes — the meaningful one).
pub fn parse_pprof(bytes: &[u8], value_index: Option<usize>) -> Result<Parsed, String> {
    let raw = maybe_gunzip(bytes)?;
    let profile = Profile::decode(&*raw).map_err(|e| format!("pprof decode: {e}"))?;

    let st = &profile.string_table;
    let get = |i: i64| -> Option<&str> {
        usize::try_from(i)
            .ok()
            .and_then(|i| st.get(i))
            .map(String::as_str)
    };

    let vi = value_index
        .filter(|&i| i < profile.sample_type.len())
        .unwrap_or_else(|| profile.sample_type.len().saturating_sub(1));

    let loc_by_id: HashMap<u64, &Location> = profile.location.iter().map(|l| (l.id, l)).collect();
    let fn_by_id: HashMap<u64, &Function> = profile.function.iter().map(|f| (f.id, f)).collect();

    let mut folded = FoldedMap::new();
    for s in &profile.sample {
        let val = s.value.get(vi).copied().unwrap_or(0);
        if val <= 0 {
            continue;
        }
        // location_id is leaf-first; each location's lines are leaf-first too.
        let mut frames: Vec<&str> = Vec::new();
        for lid in &s.location_id {
            if let Some(loc) = loc_by_id.get(lid) {
                for line in &loc.line {
                    if let Some(f) = fn_by_id.get(&line.function_id) {
                        if let Some(name) = get(f.name).filter(|n| !n.is_empty()) {
                            frames.push(name);
                        }
                    }
                }
            }
        }
        if frames.is_empty() {
            continue;
        }
        frames.reverse(); // leaf-first → leaf-last (root-first)
        *folded.entry(frames.join(";")).or_insert(0) += val;
    }

    let type_name = profile
        .sample_type
        .get(vi)
        .and_then(|vt| get(vt.r#type))
        .map(str::to_string);

    Ok(Parsed {
        folded,
        type_name,
        period_ns: profile.period,
        duration_ns: profile.duration_nanos,
    })
}

fn maybe_gunzip(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        use std::io::Read;
        // Cap the inner inflate at the same 64 MiB ceiling the outer HTTP-layer
        // decompressor uses — an uncapped read_to_end here lets a ~64 MiB gzip
        // bomb inflate to tens of GiB and OOM the process. Read one byte past
        // the cap so a body exactly at the limit passes while an over-limit one
        // is rejected, not silently truncated.
        const MAX: usize = crate::ingest_util::MAX_DECOMPRESSED;
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(bytes)
            .take(MAX as u64 + 1)
            .read_to_end(&mut out)
            .map_err(|_| "corrupt gzip pprof".to_string())?;
        if out.len() > MAX {
            return Err("decompressed pprof too large".to_string());
        }
        Ok(out)
    } else {
        Ok(bytes.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maybe_gunzip_rejects_decompression_bomb() {
        use std::io::Write;
        // A tiny gzip that inflates past the 64 MiB cap must be rejected, not
        // OOM the process. Compress (MAX+1) zero bytes → small input, over-cap
        // output.
        let bomb_len = crate::ingest_util::MAX_DECOMPRESSED + 1;
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        enc.write_all(&vec![0u8; bomb_len]).unwrap();
        let gz = enc.finish().unwrap();
        assert!(gz.len() < 1024 * 1024, "bomb input should be tiny");
        let err = maybe_gunzip(&gz).unwrap_err();
        assert!(
            err.contains("too large"),
            "expected size rejection, got: {err}"
        );

        // A small, legitimate gzip still round-trips.
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        enc.write_all(b"hello pprof").unwrap();
        let small = enc.finish().unwrap();
        assert_eq!(maybe_gunzip(&small).unwrap(), b"hello pprof");
    }

    // Build a tiny pprof by hand: two samples sharing a root frame, encode it,
    // and assert the folded output + metadata.
    #[test]
    fn decodes_pprof_to_folded() {
        // string_table[0] must be "" per spec; then frame names.
        let profile = Profile {
            string_table: vec![
                "".into(),
                "cpu".into(),
                "main".into(),
                "work".into(),
                "idle".into(),
            ],
            sample_type: vec![ValueType { r#type: 1, unit: 0 }], // "cpu"
            function: vec![
                Function { id: 1, name: 2 }, // main
                Function { id: 2, name: 3 }, // work
                Function { id: 3, name: 4 }, // idle
            ],
            location: vec![
                Location {
                    id: 10,
                    line: vec![Line { function_id: 1 }],
                }, // main
                Location {
                    id: 11,
                    line: vec![Line { function_id: 2 }],
                }, // work
                Location {
                    id: 12,
                    line: vec![Line { function_id: 3 }],
                }, // idle
            ],
            sample: vec![
                // leaf-first: work <- main  (value 7)
                Sample {
                    location_id: vec![11, 10],
                    value: vec![7],
                },
                // leaf-first: idle <- main  (value 3)
                Sample {
                    location_id: vec![12, 10],
                    value: vec![3],
                },
            ],
            period: 1000,
            duration_nanos: 5000,
            period_type: None,
        };
        let mut buf = Vec::new();
        profile.encode(&mut buf).unwrap();

        let parsed = parse_pprof(&buf, None).unwrap();
        assert_eq!(parsed.folded.get("main;work"), Some(&7));
        assert_eq!(parsed.folded.get("main;idle"), Some(&3));
        assert_eq!(parsed.type_name.as_deref(), Some("cpu"));
        assert_eq!(parsed.period_ns, 1000);
        assert_eq!(parsed.duration_ns, 5000);
    }

    #[test]
    fn gzipped_pprof_is_inflated() {
        use std::io::Write;
        let profile = Profile {
            string_table: vec!["".into(), "f".into()],
            sample_type: vec![ValueType { r#type: 0, unit: 0 }],
            function: vec![Function { id: 1, name: 1 }],
            location: vec![Location {
                id: 1,
                line: vec![Line { function_id: 1 }],
            }],
            sample: vec![Sample {
                location_id: vec![1],
                value: vec![4],
            }],
            period: 0,
            duration_nanos: 0,
            period_type: None,
        };
        let mut raw = Vec::new();
        profile.encode(&mut raw).unwrap();
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(&raw).unwrap();
        let gz = enc.finish().unwrap();

        let parsed = parse_pprof(&gz, None).unwrap();
        assert_eq!(parsed.folded.get("f"), Some(&4));
    }
}
