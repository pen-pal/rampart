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
use rampart_db::store::Store;
use std::io::Read;
use std::sync::Arc;

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
/// `pub(crate)` so the snappy (prom_write) and inner-gzip (pprof) decompressors
/// — which don't go through [`inflate_capped`] — can enforce the same ceiling.
pub(crate) const MAX_DECOMPRESSED: usize = 64 * 1024 * 1024;

/// Hard cap on the number of records (log lines / spans) accepted in one ingest
/// request. The 64 MiB body limit still permits millions of tiny rows in a
/// single payload (e.g. ~16M 4-byte syslog lines), which would balloon one `Vec`
/// and one INSERT/UNNEST transaction; this bounds the per-request row work.
/// Generous versus a typical batch (~10k); a larger producer must split across
/// requests.
pub(crate) const MAX_INGEST_RECORDS: usize = 250_000;

/// Reject a parsed batch that exceeds [`MAX_INGEST_RECORDS`] with `413` rather
/// than materializing millions of rows in one transaction.
pub(crate) fn enforce_record_cap(count: usize) -> Result<(), ApiError> {
    if count > MAX_INGEST_RECORDS {
        return Err(ApiError::PayloadTooLarge(format!(
            "ingest batch of {count} records exceeds the per-request limit of \
             {MAX_INGEST_RECORDS}; split into smaller batches"
        )));
    }
    Ok(())
}

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
pub async fn configured_token(store: &Arc<dyn Store>) -> Result<Option<String>, ApiError> {
    let raw = store.get_setting(TELEMETRY_TOKEN_KEY).await?;
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
pub async fn sampling_config(store: &Arc<dyn Store>) -> Result<SamplingConfig, ApiError> {
    let raw = store.get_setting(SAMPLING_KEY).await?;
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
    store: &Arc<dyn Store>,
    headers: &HeaderMap,
    query_k: Option<&str>,
) -> Result<(), ApiError> {
    let configured = configured_token(store).await?;
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
///
/// When `RAMPART_MULTI_ORG` is set (Phase 6 enforcement), a key MISS is a hard
/// `Unauthorized` instead — no Default fallback — so un-keyed / wrong-keyed
/// telemetry cannot land in (or be read from) the Default org. See
/// [`multi_org_enabled`].
/// The ingest surface a request hit, matched against the resolved key's `kind`
/// scope. A key with `kind = "all"` accepts every surface; any other kind only
/// accepts its own. (`Syslog` has no mintable kind today, so a syslog forwarder
/// needs an `all` key.)
#[derive(Clone, Copy)]
pub enum IngestSurface {
    Otlp,
    Prometheus,
    Rum,
    Profiles,
    Syslog,
}

impl IngestSurface {
    fn as_kind(self) -> &'static str {
        match self {
            IngestSurface::Otlp => "otlp",
            IngestSurface::Prometheus => "prometheus",
            IngestSurface::Rum => "rum",
            IngestSurface::Profiles => "profiles",
            IngestSurface::Syslog => "syslog",
        }
    }
}

/// Does a key's `kind` scope permit this surface? `all` permits everything;
/// otherwise the kind must match the surface exactly.
fn kind_allows(key_kind: &str, surface: IngestSurface) -> bool {
    key_kind == "all" || key_kind == surface.as_kind()
}

pub async fn resolve_ingest_org(
    store: &Arc<dyn Store>,
    headers: &HeaderMap,
    query_k: Option<&str>,
    surface: IngestSurface,
) -> Result<rampart_core::ids::OrgId, ApiError> {
    resolve_ingest(store, headers, query_k, None, surface).await
}

/// Like [`resolve_ingest_org`], but ALSO origin-binds the key (Phase 5-3 RUM).
/// The RUM `?k` token necessarily ships in the browser snippet (public,
/// forgeable), so when the matched ingest key carries `allowed_origins` the
/// request's `Origin` MUST be in that list — otherwise a leaked/forged key
/// can't misattribute beacons to another org from a different site. A key with
/// no `allowed_origins` is unrestricted (same as the plain resolver).
pub async fn resolve_ingest_org_origin(
    store: &Arc<dyn Store>,
    headers: &HeaderMap,
    query_k: Option<&str>,
    origin: Option<&str>,
    surface: IngestSurface,
) -> Result<rampart_core::ids::OrgId, ApiError> {
    resolve_ingest(store, headers, query_k, Some(origin), surface).await
}

/// Shared resolver. `enforce_origin`: `None` = don't origin-bind (OTLP / prom /
/// profiles — no browser Origin); `Some(origin)` = enforce `allowed_origins`
/// against `origin` when the key restricts origins (RUM).
async fn resolve_ingest(
    store: &Arc<dyn Store>,
    headers: &HeaderMap,
    query_k: Option<&str>,
    enforce_origin: Option<Option<&str>>,
    surface: IngestSurface,
) -> Result<rampart_core::ids::OrgId, ApiError> {
    if let Some(tok) = presented_token(headers, query_k) {
        if let Some((id, org_id, kind, allowed)) = store.find_ingest_key_by_token(tok).await? {
            // Surface scope: a key minted for one signal (e.g. a RUM key, which
            // ships publicly in browser JS) must not write to a different surface
            // (OTLP/prom/profiles). `all` keys accept everything.
            if !kind_allows(&kind, surface) {
                return Err(ApiError::Forbidden);
            }
            if let Some(origin) = enforce_origin {
                if !allowed.is_empty() {
                    let o = origin.unwrap_or("");
                    if !allowed.iter().any(|a| a == o) {
                        return Err(ApiError::Unauthorized);
                    }
                }
            }
            let _ = store.touch_ingest_key_last_used(id).await;
            return Ok(org_id);
        }
    }
    // Key miss. In multi-org mode every ingest request MUST carry a valid
    // per-org ingest key — there is NO silent Default fallback, because that
    // would let un-keyed or wrong-keyed telemetry land in (and then be read
    // from) the Default org. Refuse outright. The flag is opt-in (default off)
    // so single-org and legacy global-token installs are byte-identical; an
    // operator flips it on only AFTER minting per-org ingest keys.
    if multi_org_enabled() {
        return Err(ApiError::Unauthorized);
    }
    // Single-org / legacy: global-token gate, verbatim, then the Default org.
    require_telemetry_token(store, headers, query_k).await?;
    Ok(rampart_core::ids::OrgId::from_uuid(
        rampart_core::org::DEFAULT_ORG_ID,
    ))
}

/// Whether ingest auth is mandatory (`RAMPART_REQUIRE_INGEST_AUTH=1`/`true`).
pub fn require_ingest_auth() -> bool {
    flag_set("RAMPART_REQUIRE_INGEST_AUTH")
}

/// Whether multi-org enforcement is on (`RAMPART_MULTI_ORG=1`/`true`). When set,
/// an ingest request whose token doesn't match a per-org `ingest_keys` row is
/// refused outright instead of silently resolving to the Default org (see
/// [`resolve_ingest`]). Opt-in; default off keeps single-org and legacy
/// global-token installs byte-for-byte identical. An operator flips this on only
/// after minting per-org ingest keys, otherwise all un-keyed ingest 401s.
pub fn multi_org_enabled() -> bool {
    flag_set("RAMPART_MULTI_ORG")
}

/// Shared truthy-env parse for the boolean ops flags above.
fn flag_set(var: &str) -> bool {
    matches!(
        std::env::var(var).as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes")
    )
}

/// Length-independent constant-time byte comparison. Mismatched lengths
/// still walk the longer buffer so timing doesn't reveal the secret length.
/// `pub(crate)` so the Sentry DSN-key check (error_ingest) shares it.
pub(crate) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
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

    /// SECURITY (audit re-rank #11): an ingest key's `kind` scope is enforced —
    /// `all` accepts any surface, a scoped key only its own (so a public RUM key
    /// can't write OTLP/prom/profiles).
    #[test]
    fn ingest_kind_scope_enforced() {
        assert!(kind_allows("all", IngestSurface::Otlp));
        assert!(kind_allows("all", IngestSurface::Rum));
        assert!(kind_allows("otlp", IngestSurface::Otlp));
        assert!(kind_allows("prometheus", IngestSurface::Prometheus));
        assert!(!kind_allows("rum", IngestSurface::Otlp));
        assert!(!kind_allows("otlp", IngestSurface::Prometheus));
        assert!(!kind_allows("profiles", IngestSurface::Syslog));
    }

    /// DoS guard (audit re-rank #9): a parsed batch over the per-request record
    /// cap is rejected with 413, not materialized into one giant insert.
    #[test]
    fn record_cap_rejects_oversize_batch() {
        assert!(enforce_record_cap(0).is_ok());
        assert!(enforce_record_cap(MAX_INGEST_RECORDS).is_ok());
        assert!(matches!(
            enforce_record_cap(MAX_INGEST_RECORDS + 1),
            Err(ApiError::PayloadTooLarge(_))
        ));
    }

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
    fn flag_set_parses_truthy_only() {
        // Unique var name so this can't race with other tests (or the real
        // RAMPART_MULTI_ORG / RAMPART_REQUIRE_INGEST_AUTH flags).
        let var = "RAMPART_TEST_FLAG_PARSE_8F2A";
        std::env::remove_var(var);
        assert!(!flag_set(var), "unset is false");
        for v in ["1", "true", "TRUE", "yes"] {
            std::env::set_var(var, v);
            assert!(flag_set(var), "{v:?} should be truthy");
        }
        for v in ["0", "false", "no", "", "maybe"] {
            std::env::set_var(var, v);
            assert!(!flag_set(var), "{v:?} should be falsey");
        }
        std::env::remove_var(var);
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
