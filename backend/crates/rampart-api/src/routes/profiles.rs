//! Profiling ingest + read API.
//!
//! Ingest (public root surface at `/profiles`, IP-rate-limited upstream,
//! optional shared telemetry token — like `/otlp` and `/rum`):
//!
//! - `POST /profiles/v1/folded` — Brendan-Gregg folded text (`stack value\n`).
//! - `POST /profiles/v1/pprof`  — gzipped pprof protobuf.
//!
//! Read (under `/v1/profiles`, in the protected tree):
//!
//! - `GET /v1/profiles`                  — list profiles in a window.
//! - `GET /v1/profiles/services`         — distinct services (picker).
//! - `GET /v1/profiles/types`            — distinct types (optional `service`).
//! - `GET /v1/profiles/flamegraph`       — merged tree for `(service,type,window)`.
//! - `GET /v1/profiles/{id}/flamegraph`  — tree for a single profile.
//!
//! Every wire format is lowered to a folded map (`rampart_core::profile`),
//! serialized to canonical text, gzipped, and stored. Flamegraphs merge the
//! folded maps in the window on read. See `docs/design/PROFILING.md`.

use crate::auth::OrgContext;
use crate::error::ApiError;
use crate::state::AppState;
use axum::body::Bytes;
use axum::extract::{Extension, Path, Query, State};
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use rampart_core::profile::{self, FlameNode, FnStat, FoldedMap};
use rampart_db::profiles::{NewProfile, ProfileMeta};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use time::OffsetDateTime;

/// Public ingest surface — mounted at `/profiles`, behind the ingest rate limit.
pub fn ingest_router() -> Router<AppState> {
    Router::new()
        .route("/v1/folded", post(ingest_folded))
        .route("/v1/pprof", post(ingest_pprof))
}

/// Protected read surface — mounted at `/v1/profiles`.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list))
        .route("/services", get(services))
        .route("/types", get(types))
        .route("/flamegraph", get(flamegraph_window))
        .route("/flamegraph/diff", get(flamegraph_diff))
        .route("/{id}/flamegraph", get(flamegraph_one))
}

// ── ingest ──────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct IngestQuery {
    service: Option<String>,
    #[serde(rename = "type")]
    profile_type: Option<String>,
    period_ns: Option<i64>,
    duration_ns: Option<i64>,
}

async fn ingest_folded(
    State(s): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<IngestQuery>,
    body: Bytes,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Phase 5: resolve the owning org from the ingest credential (auth-gated
    // internally; Default on key-miss).
    let org = crate::ingest_util::resolve_ingest_org(s.pool(), &headers, None).await?;
    // Honor Content-Encoding (gzip/deflate) like the OTLP path.
    let body = crate::ingest_util::decompress(&headers, &body)?;
    let text = String::from_utf8(body)
        .map_err(|_| ApiError::BadRequest("folded body must be UTF-8".into()))?;
    let map = profile::parse_folded(&text);
    if map.is_empty() {
        return Err(ApiError::BadRequest(
            "no valid folded-stack lines (expected `frame;frame value` per line)".into(),
        ));
    }
    let service = non_empty(q.service.as_deref()).unwrap_or("unknown");
    let profile_type = non_empty(q.profile_type.as_deref()).unwrap_or("cpu");
    store(
        &s,
        service,
        profile_type,
        q.period_ns.unwrap_or(0),
        q.duration_ns.unwrap_or(0),
        map,
        org,
    )
    .await?;
    Ok(Json(serde_json::json!({})))
}

async fn ingest_pprof(
    State(s): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<IngestQuery>,
    body: Bytes,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Phase 5: resolve the owning org from the ingest credential (auth-gated
    // internally; Default on key-miss).
    let org = crate::ingest_util::resolve_ingest_org(s.pool(), &headers, None).await?;
    // Strip any HTTP Content-Encoding first; the pprof payload is itself usually
    // gzipped (parse_pprof inflates that inner layer).
    let body = crate::ingest_util::decompress(&headers, &body)?;
    let parsed = crate::pprof::parse_pprof(&body, None).map_err(ApiError::BadRequest)?;
    if parsed.folded.is_empty() {
        return Err(ApiError::BadRequest(
            "pprof contained no samples with a positive value".into(),
        ));
    }
    // Type precedence: explicit query param, else the pprof sample-type name,
    // else "cpu".
    let profile_type = non_empty(q.profile_type.as_deref())
        .or(parsed.type_name.as_deref())
        .unwrap_or("cpu");
    let service = non_empty(q.service.as_deref()).unwrap_or("unknown");
    store(
        &s,
        service,
        profile_type,
        parsed.period_ns,
        parsed.duration_ns,
        parsed.folded,
        org,
    )
    .await?;
    Ok(Json(serde_json::json!({})))
}

/// OTLP profiles ingest. Mounted on the `/otlp` surface (so an OTLP/HTTP
/// exporter pointed at `http://host/otlp` reaches `…/v1development/profiles`).
/// One request can carry many profiles (multiple resources/scopes); each is
/// lowered and stored independently.
pub async fn ingest_otlp(
    State(s): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Phase 5: resolve the owning org from the ingest credential (auth-gated
    // internally; Default on key-miss).
    let org = crate::ingest_util::resolve_ingest_org(s.pool(), &headers, None).await?;
    let body = crate::ingest_util::decompress(&headers, &body)?;
    let profiles =
        crate::otlp_profiles::parse_otlp_profiles(&body).map_err(ApiError::BadRequest)?;
    for p in profiles {
        if p.folded.is_empty() {
            continue;
        }
        let profile_type = non_empty(p.type_name.as_deref()).unwrap_or("cpu");
        store(
            &s,
            &p.service,
            profile_type,
            p.period_ns,
            p.duration_ns,
            p.folded,
            org,
        )
        .await?;
    }
    Ok(Json(serde_json::json!({})))
}

/// Lower a folded map to storage: canonical text → gzip → insert. Shared by
/// every ingest format — folded text, pprof, and OTLP profiles.
async fn store(
    s: &AppState,
    service: &str,
    profile_type: &str,
    period_ns: i64,
    duration_ns: i64,
    map: FoldedMap,
    org: rampart_core::ids::OrgId,
) -> Result<(), ApiError> {
    let sample_count = map.values().sum::<i64>().clamp(0, i32::MAX as i64) as i32;
    let gz = gzip(profile::to_folded_text(&map).as_bytes())?;
    // RLS: bind the resolved org around the insert (the single write point for
    // all three ingest formats). No-op when RAMPART_RLS is off.
    rampart_db::rls::with_org(org, async move {
        rampart_db::profiles::insert(
            s.pool(),
            NewProfile {
                service_name: service,
                profile_type,
                period_ns,
                duration_ns,
                sample_count,
                labels: serde_json::json!({}),
                folded_gz: &gz,
            },
            org,
        )
        .await?;
        Ok(())
    })
    .await
}

// ── read ────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ListQuery {
    service: Option<String>,
    #[serde(rename = "type")]
    profile_type: Option<String>,
    hours: Option<i32>,
}

async fn list(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<ProfileMeta>>, ApiError> {
    Ok(Json(
        rampart_db::profiles::list(
            s.pool(),
            non_empty(q.service.as_deref()),
            non_empty(q.profile_type.as_deref()),
            q.hours.unwrap_or(24),
            200,
            org.org_id,
        )
        .await?,
    ))
}

async fn services(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
) -> Result<Json<Vec<String>>, ApiError> {
    Ok(Json(
        rampart_db::profiles::services(s.pool(), org.org_id).await?,
    ))
}

#[derive(Deserialize)]
struct TypesQuery {
    service: Option<String>,
}

async fn types(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Query(q): Query<TypesQuery>,
) -> Result<Json<Vec<String>>, ApiError> {
    Ok(Json(
        rampart_db::profiles::profile_types(s.pool(), non_empty(q.service.as_deref()), org.org_id)
            .await?,
    ))
}

/// The flamegraph payload: the inclusive-value tree plus the tabular top-
/// functions companion and the merged sample total.
#[derive(Serialize)]
struct FlameResponse {
    tree: FlameNode,
    top: Vec<FnStat>,
    sample_count: i64,
}

#[derive(Deserialize)]
struct FlameQuery {
    service: String,
    #[serde(rename = "type")]
    profile_type: String,
    hours: Option<i32>,
    /// Absolute window (epoch ms) — the trace→profile pivot passes a span's
    /// exact [start, end] here so the flamegraph is scoped to that span. When
    /// both are present they take precedence over `hours`.
    from_ms: Option<i64>,
    to_ms: Option<i64>,
}

async fn flamegraph_window(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Query(q): Query<FlameQuery>,
) -> Result<Json<FlameResponse>, ApiError> {
    let (from, to) = match (q.from_ms, q.to_ms) {
        (Some(a), Some(b)) if b > a => {
            let from = OffsetDateTime::from_unix_timestamp_nanos(a as i128 * 1_000_000)
                .map_err(|_| ApiError::BadRequest("invalid from_ms".into()))?;
            let to = OffsetDateTime::from_unix_timestamp_nanos(b as i128 * 1_000_000)
                .map_err(|_| ApiError::BadRequest("invalid to_ms".into()))?;
            // Cap the absolute window so a bad client can't request a year.
            let from = from.max(to - time::Duration::days(90));
            (from, to)
        }
        _ => {
            let hours = q.hours.unwrap_or(24).clamp(1, 24 * 90);
            let to = OffsetDateTime::now_utc();
            (to - time::Duration::hours(hours as i64), to)
        }
    };
    let blobs = rampart_db::profiles::folded_in_window(
        s.pool(),
        &q.service,
        &q.profile_type,
        from,
        to,
        org.org_id,
    )
    .await?;
    let merged = merge_blobs(&blobs)?;
    Ok(Json(build_response(&q.service, merged)))
}

/// A diff flamegraph: the after-window tree annotated with after−before deltas,
/// plus both windows' sample totals.
#[derive(Serialize)]
struct DiffResponse {
    tree: profile::DiffNode,
    after_samples: i64,
    before_samples: i64,
}

async fn flamegraph_diff(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Query(q): Query<FlameQuery>,
) -> Result<Json<DiffResponse>, ApiError> {
    let hours = q.hours.unwrap_or(24).clamp(1, 24 * 90);
    let span = time::Duration::hours(hours as i64);
    let now = OffsetDateTime::now_utc();
    // after = the most recent window; before = the window immediately preceding.
    let after = merge_blobs(
        &rampart_db::profiles::folded_in_window(
            s.pool(),
            &q.service,
            &q.profile_type,
            now - span,
            now,
            org.org_id,
        )
        .await?,
    )?;
    let before = merge_blobs(
        &rampart_db::profiles::folded_in_window(
            s.pool(),
            &q.service,
            &q.profile_type,
            now - span - span,
            now - span,
            org.org_id,
        )
        .await?,
    )?;
    Ok(Json(DiffResponse {
        after_samples: after.values().sum(),
        before_samples: before.values().sum(),
        tree: profile::fold_to_diff_tree(&before, &after, &q.service),
    }))
}

async fn flamegraph_one(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Path(id): Path<i64>,
) -> Result<Json<FlameResponse>, ApiError> {
    let Some((profile_type, gz)) =
        rampart_db::profiles::fetch_folded(s.pool(), id, org.org_id).await?
    else {
        return Err(ApiError::NotFound);
    };
    let map = parse_blob(&gz)?;
    Ok(Json(build_response(&profile_type, map)))
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn build_response(root: &str, map: FoldedMap) -> FlameResponse {
    FlameResponse {
        sample_count: map.values().sum(),
        tree: profile::fold_to_tree(&map, root),
        top: profile::top_functions(&map, 25),
    }
}

/// Gunzip + parse one stored folded blob into a map.
fn parse_blob(gz: &[u8]) -> Result<FoldedMap, ApiError> {
    let text = String::from_utf8(gunzip(gz)?)
        .map_err(|_| ApiError::Internal(anyhow::anyhow!("stored profile is not UTF-8")))?;
    Ok(profile::parse_folded(&text))
}

/// Merge every stored blob in a window into one folded map.
fn merge_blobs(blobs: &[Vec<u8>]) -> Result<FoldedMap, ApiError> {
    let mut acc = FoldedMap::new();
    for b in blobs {
        profile::merge_into(&mut acc, &parse_blob(b)?);
    }
    Ok(acc)
}

fn non_empty(s: Option<&str>) -> Option<&str> {
    s.filter(|x| !x.is_empty())
}

fn gzip(data: &[u8]) -> Result<Vec<u8>, ApiError> {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(data)
        .map_err(|e| ApiError::Internal(e.into()))?;
    e.finish().map_err(|e| ApiError::Internal(e.into()))
}

fn gunzip(data: &[u8]) -> Result<Vec<u8>, ApiError> {
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(data)
        .read_to_end(&mut out)
        .map_err(|_| ApiError::BadRequest("corrupt gzip body".into()))?;
    Ok(out)
}
