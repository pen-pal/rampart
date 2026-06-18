//! Shared helpers for the public ingest surfaces (OTLP, RUM, Sentry).
//!
//! Two concerns live here because every external-telemetry endpoint needs
//! them and they were previously duplicated:
//!
//!  - [`decompress`] — transparent `Content-Encoding: gzip|deflate` decode.
//!    Stock OpenTelemetry SDKs/Collectors gzip their OTLP/HTTP exports by
//!    default and Sentry SDKs gzip envelopes, so an ingest endpoint that
//!    only reads the raw body silently rejects real-world traffic.
//!
//!  - [`require_telemetry_token`] — optional shared-secret auth for the
//!    root-level ingest endpoints. Single-tenant Rampart leans on the
//!    operator controlling network exposure, so auth is *opt-in*: if the
//!    `telemetry_token` setting is empty/unset the endpoints stay open
//!    (unchanged behaviour); once the operator sets a token, OTLP + RUM
//!    ingest require it. The token is matched against, in order, an
//!    `Authorization: Bearer <tok>` header, an `X-Rampart-Token` header,
//!    or a `?k=<tok>` query param (the last is for `navigator.sendBeacon`,
//!    which can't set request headers).

use crate::error::ApiError;
use axum::http::HeaderMap;
use rampart_db::DbPool;
use std::io::Read;

/// Setting key holding the optional shared ingest secret (a JSON string).
pub const TELEMETRY_TOKEN_KEY: &str = "telemetry_token";

/// Decode `body` per its `Content-Encoding` header. Unknown/absent encodings
/// pass through unchanged; `gzip` and `deflate` (zlib) are inflated.
pub fn decompress(headers: &HeaderMap, body: &[u8]) -> Result<Vec<u8>, ApiError> {
    let enc = headers
        .get("content-encoding")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    if enc.contains("gzip") {
        inflate_capped(flate2::read::GzDecoder::new(body), "gzip")
    } else if enc.contains("deflate") {
        inflate_capped(flate2::read::ZlibDecoder::new(body), "deflate")
    } else {
        Ok(body.to_vec())
    }
}

/// Hard ceiling on decompressed body size. A few KB of crafted gzip can inflate
/// to gigabytes ("zip bomb"); without a cap, `read_to_end` would happily OOM
/// the process. 64 MiB is far above any legitimate single OTLP/Sentry/RUM batch.
const MAX_DECOMPRESSED: usize = 64 * 1024 * 1024;

/// Inflate `reader` into memory, refusing anything past [`MAX_DECOMPRESSED`].
/// Reads one byte past the cap so a body exactly at the limit still passes
/// while an over-limit body is rejected rather than silently truncated.
fn inflate_capped<R: Read>(reader: R, label: &str) -> Result<Vec<u8>, ApiError> {
    let mut out = Vec::new();
    reader
        .take(MAX_DECOMPRESSED as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|_| ApiError::BadRequest(format!("invalid {label} body")))?;
    if out.len() > MAX_DECOMPRESSED {
        return Err(ApiError::BadRequest("decompressed body too large".into()));
    }
    Ok(out)
}

/// Read the configured telemetry token, if any. `None` means auth is
/// disabled (the endpoints stay open). An empty-string setting is treated
/// the same as unset so clearing the field in the UI re-opens the surface.
pub async fn configured_token(pool: &DbPool) -> Result<Option<String>, ApiError> {
    let raw = rampart_db::settings::get(pool, TELEMETRY_TOKEN_KEY).await?;
    Ok(raw
        .and_then(|v| v.as_str().map(str::to_owned))
        .filter(|s| !s.is_empty()))
}

/// Head-sampling keep-percentages for the high-volume ingest tiers, stored in
/// `settings.ingest_sampling`. 100 = keep everything (the default = sampling
/// off). Read once per ingest batch; a batch is many records, so the lookup
/// amortises.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SamplingConfig {
    #[serde(default = "full_pct")]
    pub traces_pct: i32,
    #[serde(default = "full_pct")]
    pub logs_pct: i32,
}

fn full_pct() -> i32 {
    100
}

impl Default for SamplingConfig {
    fn default() -> Self {
        Self {
            traces_pct: 100,
            logs_pct: 100,
        }
    }
}

pub const SAMPLING_KEY: &str = "ingest_sampling";

/// Load the sampling config, clamped to 0..=100. Missing/garbage = off.
pub async fn sampling_config(pool: &DbPool) -> Result<SamplingConfig, ApiError> {
    let raw = rampart_db::settings::get(pool, SAMPLING_KEY).await?;
    let mut cfg: SamplingConfig = raw
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    cfg.traces_pct = cfg.traces_pct.clamp(0, 100);
    cfg.logs_pct = cfg.logs_pct.clamp(0, 100);
    Ok(cfg)
}

/// Extract the caller-presented token from the request: `Authorization:
/// Bearer <tok>`, then `X-Rampart-Token: <tok>`, then `?k=<tok>`.
pub fn presented_token<'a>(headers: &'a HeaderMap, query_k: Option<&'a str>) -> Option<&'a str> {
    if let Some(bearer) = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.strip_prefix("Bearer ")
                .or_else(|| v.strip_prefix("bearer "))
        })
    {
        let bearer = bearer.trim();
        if !bearer.is_empty() {
            return Some(bearer);
        }
    }
    if let Some(h) = headers.get("x-rampart-token").and_then(|v| v.to_str().ok()) {
        let h = h.trim();
        if !h.is_empty() {
            return Some(h);
        }
    }
    query_k.map(str::trim).filter(|s| !s.is_empty())
}

/// Enforce telemetry auth. With no token configured, ingest is open unless
/// `RAMPART_REQUIRE_INGEST_AUTH` is set — then a token MUST be configured and
/// presented (an open ingest surface is refused outright). With a token
/// configured it is always enforced. Comparison is constant-time.
pub async fn require_telemetry_token(
    pool: &DbPool,
    headers: &HeaderMap,
    query_k: Option<&str>,
) -> Result<(), ApiError> {
    let configured = configured_token(pool).await?;
    let Some(expected) = configured else {
        // No token set: open by default; refuse if the operator mandated auth.
        return if require_ingest_auth() {
            Err(ApiError::Unauthorized)
        } else {
            Ok(())
        };
    };
    let presented = presented_token(headers, query_k).unwrap_or("");
    if constant_time_eq(presented.as_bytes(), expected.as_bytes()) {
        Ok(())
    } else {
        Err(ApiError::Unauthorized)
    }
}

/// Resolve the owning ORG for a shared-token ingest request (multi-tenancy
/// Phase 5). The presented token (Bearer / `X-Rampart-Token` / `?k`) is looked
/// up in `ingest_keys`: a hit returns that key's org (and bumps last-used). A
/// MISS falls through to [`require_telemetry_token`] VERBATIM — preserving the
/// legacy open-by-default / `RAMPART_REQUIRE_INGEST_AUTH` / constant-time
/// global-token semantics exactly — and lands on the Default org. So a
/// single-org install with no minted keys (or an existing global-token install)
/// is byte-for-byte behaviour-identical; per-org isolation activates only once
/// an operator mints org-scoped ingest keys.
pub async fn resolve_ingest_org(
    pool: &DbPool,
    headers: &HeaderMap,
    query_k: Option<&str>,
) -> Result<rampart_core::ids::OrgId, ApiError> {
    if let Some(tok) = presented_token(headers, query_k) {
        if let Some((id, org_id, _origins)) = rampart_db::ingest_keys::find_by_token(pool, tok).await?
        {
            let _ = rampart_db::ingest_keys::touch_last_used(pool, id).await;
            return Ok(org_id);
        }
    }
    // Key miss → legacy global-token gate, verbatim, then the Default org.
    require_telemetry_token(pool, headers, query_k).await?;
    Ok(rampart_core::ids::OrgId::from_uuid(
        rampart_core::org::DEFAULT_ORG_ID,
    ))
}

/// Whether ingest auth is mandatory (`RAMPART_REQUIRE_INGEST_AUTH=1`/`true`).
pub fn require_ingest_auth() -> bool {
    matches!(
        std::env::var("RAMPART_REQUIRE_INGEST_AUTH").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes")
    )
}

/// Length-independent constant-time byte comparison. Mismatched lengths
/// still walk the longer buffer so timing doesn't reveal the secret length.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = (a.len() ^ b.len()) as u8;
    let n = a.len().max(b.len());
    for i in 0..n {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, HeaderValue};
    use std::io::Write;

    #[test]
    fn decompress_passthrough_when_unencoded() {
        let h = HeaderMap::new();
        assert_eq!(decompress(&h, b"hello").unwrap(), b"hello");
    }

    #[test]
    fn decompress_gzip_roundtrip() {
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(b"payload-body").unwrap();
        let gz = enc.finish().unwrap();
        let mut h = HeaderMap::new();
        h.insert("content-encoding", HeaderValue::from_static("gzip"));
        assert_eq!(decompress(&h, &gz).unwrap(), b"payload-body");
    }

    #[test]
    fn decompress_deflate_roundtrip() {
        let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(b"deflated").unwrap();
        let z = enc.finish().unwrap();
        let mut h = HeaderMap::new();
        h.insert("content-encoding", HeaderValue::from_static("deflate"));
        assert_eq!(decompress(&h, &z).unwrap(), b"deflated");
    }

    #[test]
    fn decompress_bad_gzip_is_bad_request() {
        let mut h = HeaderMap::new();
        h.insert("content-encoding", HeaderValue::from_static("gzip"));
        assert!(decompress(&h, b"not-actually-gzip").is_err());
    }

    #[test]
    fn presented_prefers_bearer_then_header_then_query() {
        let mut h = HeaderMap::new();
        h.insert("authorization", HeaderValue::from_static("Bearer abc"));
        assert_eq!(presented_token(&h, Some("zzz")), Some("abc"));

        let mut h = HeaderMap::new();
        h.insert("x-rampart-token", HeaderValue::from_static("hdr"));
        assert_eq!(presented_token(&h, Some("zzz")), Some("hdr"));

        let h = HeaderMap::new();
        assert_eq!(presented_token(&h, Some("qry")), Some("qry"));
        assert_eq!(presented_token(&h, None), None);
        assert_eq!(presented_token(&h, Some("")), None);
    }

    #[test]
    fn constant_time_eq_matches_std_eq() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(!constant_time_eq(b"secret", b"secrxt"));
        assert!(!constant_time_eq(b"secret", b"secre"));
        assert!(!constant_time_eq(b"", b"x"));
        assert!(constant_time_eq(b"", b""));
    }
}
