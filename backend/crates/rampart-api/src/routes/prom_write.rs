//! Prometheus `remote_write` ingest — turns Rampart into a metrics sink, not
//! just a `/metrics` source. Prometheus POSTs a snappy-compressed protobuf
//! `WriteRequest` to `/prom/write`; we decode it and store the latest sample of
//! each series in the same `metric_samples` tier the scrape path uses.
//!
//! The wire types are a hand-written prost subset of `prometheus.WriteRequest`
//! (same approach as the OTLP ingest) so we don't pull the full Prometheus
//! proto crate.

use crate::error::ApiError;
use crate::state::AppState;
use axum::body::Bytes;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::Router;
use prost::Message;
use rampart_core::promtext::PromSample;
use std::collections::BTreeMap;

pub fn router() -> Router<AppState> {
    Router::new().route("/write", post(remote_write))
}

#[derive(Clone, PartialEq, Message)]
struct WriteRequest {
    #[prost(message, repeated, tag = "1")]
    timeseries: Vec<TimeSeries>,
}

#[derive(Clone, PartialEq, Message)]
struct TimeSeries {
    #[prost(message, repeated, tag = "1")]
    labels: Vec<Label>,
    #[prost(message, repeated, tag = "2")]
    samples: Vec<Sample>,
}

#[derive(Clone, PartialEq, Message)]
struct Label {
    #[prost(string, tag = "1")]
    name: String,
    #[prost(string, tag = "2")]
    value: String,
}

#[derive(Clone, PartialEq, Message)]
struct Sample {
    #[prost(double, tag = "1")]
    value: f64,
    #[prost(int64, tag = "2")]
    timestamp: i64,
}

async fn remote_write(
    axum::extract::State(s): axum::extract::State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    // Phase 5: resolve the owning org from the ingest credential (same optional
    // shared-token gate as before, now org-aware; Default on key-miss).
    let org = crate::ingest_util::resolve_ingest_org(
        s.pool(),
        &headers,
        None,
        crate::ingest_util::IngestSurface::Prometheus,
    )
    .await?;
    // RLS: bind the resolved org for the write below (no-op when RAMPART_RLS off).
    rampart_db::rls::with_org(org, async move {
        // remote_write bodies are snappy *block* format (not framed). The block
        // header carries an attacker-declared decompressed length; `decompress_vec`
        // eagerly allocates that much (capped only by snap's ~4 GiB limit) BEFORE
        // any validation, so a tiny crafted POST can trigger a multi-GiB allocation
        // and OOM the process. Reject by the declared length first — same 64 MiB
        // ceiling the gzip/deflate paths enforce.
        let declared = snap::raw::decompress_len(&body)
            .map_err(|e| ApiError::BadRequest(format!("invalid snappy: {e}")))?;
        if declared > crate::ingest_util::MAX_DECOMPRESSED {
            return Err(ApiError::BadRequest("decompressed body too large".into()));
        }
        let raw = snap::raw::Decoder::new()
            .decompress_vec(&body)
            .map_err(|e| ApiError::BadRequest(format!("invalid snappy: {e}")))?;
        let req = WriteRequest::decode(&raw[..])
            .map_err(|e| ApiError::BadRequest(format!("invalid remote_write protobuf: {e}")))?;

        let mut samples = Vec::new();
        for ts in req.timeseries {
            // __name__ is the metric name; the rest are dimensions. Skip a series
            // with no name (malformed) and series with no samples.
            let mut name = String::new();
            let mut labels = BTreeMap::new();
            for l in ts.labels {
                if l.name == "__name__" {
                    name = l.value;
                } else {
                    labels.insert(l.name, l.value);
                }
            }
            if name.is_empty() {
                continue;
            }
            if let Some(last) = ts.samples.last() {
                // NaN = a "stale" marker in remote_write; drop it.
                if last.value.is_nan() {
                    continue;
                }
                samples.push(PromSample {
                    name,
                    labels,
                    value: last.value,
                });
            }
        }

        s.store().insert_metric_samples(&samples, org).await?;
        Ok(StatusCode::NO_CONTENT)
    })
    .await
}
