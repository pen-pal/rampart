//! Syslog + JSON-lines log ingest.
//!
//! - `POST /syslog` — text body, one or more newline-framed RFC 5424 / RFC 3164
//!   lines (the wire format rsyslog/syslog-ng emit).
//! - `POST /syslog/json` — NDJSON, one JSON log object per line.
//!
//! Public, rate-limited ingest surface mounted alongside `/otlp` and `/prom`.
//! Auth + org resolution reuse the shared `resolve_ingest_org` token path
//! (Bearer / `X-Rampart-Token` / `?k=`), and writes land in the existing `logs`
//! table via `store().insert_logs` — so syslog rows flow into the detection
//! engine exactly like OTLP logs, with no schema change.

use crate::error::ApiError;
use crate::state::AppState;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};
use rampart_core::log::ParsedLog;
use serde::Deserialize;
use serde_json::Value;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", post(ingest_text))
        .route("/json", post(ingest_json))
}

#[derive(Deserialize)]
struct TokenQuery {
    /// Optional ingest token in the URL (`?k=`), same as the other ingest
    /// surfaces — convenient for forwarders that can't set a header.
    k: Option<String>,
}

/// Newline-framed RFC 5424 / RFC 3164 ingest.
async fn ingest_text(
    State(s): State<AppState>,
    Query(q): Query<TokenQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let org = crate::ingest_util::resolve_ingest_org(s.pool(), &headers, q.k.as_deref()).await?;
    rampart_db::rls::with_org(org, async move {
        let body = crate::ingest_util::decompress(&headers, &body)?;
        let text = String::from_utf8_lossy(&body);
        let logs = rampart_core::syslog::parse_syslog(&text);
        crate::ingest_util::enforce_record_cap(logs.len())?;
        let logs = sample_logs(&s, logs).await?;
        s.store().insert_logs(&logs, org).await?;
        Ok(Json(serde_json::json!({ "accepted": logs.len() })))
    })
    .await
}

/// NDJSON (one JSON log object per line) ingest.
async fn ingest_json(
    State(s): State<AppState>,
    Query(q): Query<TokenQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let org = crate::ingest_util::resolve_ingest_org(s.pool(), &headers, q.k.as_deref()).await?;
    rampart_db::rls::with_org(org, async move {
        let body = crate::ingest_util::decompress(&headers, &body)?;
        let text = String::from_utf8_lossy(&body);
        let logs = rampart_core::syslog::parse_ndjson(&text);
        crate::ingest_util::enforce_record_cap(logs.len())?;
        let logs = sample_logs(&s, logs).await?;
        s.store().insert_logs(&logs, org).await?;
        Ok(Json(serde_json::json!({ "accepted": logs.len() })))
    })
    .await
}

/// Apply the shared logs head-sampling (same `logs_pct` + key scheme as the
/// OTLP log path) so syslog respects the configured sampling rate.
async fn sample_logs(s: &AppState, mut logs: Vec<ParsedLog>) -> Result<Vec<ParsedLog>, ApiError> {
    let sc = crate::ingest_util::sampling_config(s.pool()).await?;
    if sc.logs_pct < 100 {
        logs.retain(|l| {
            let key = l
                .trace_id
                .clone()
                .unwrap_or_else(|| format!("{}:{}:{}", l.service_name, l.time_ns, l.body));
            rampart_core::sampling::keep(&key, sc.logs_pct)
        });
    }
    Ok(logs)
}
