//! HTTP / HTTPS probe.
//!
//! Handles three monitor kinds with one probe:
//! - Http        — status code matches `accepted_statuses`
//! - Keyword     — `accepted_statuses` AND body contains `config.keyword`
//! - JsonQuery   — `accepted_statuses` AND `config.json_path` returns a
//!   value equal to `config.expected_value` (simplest form — full JSONPath
//!   comes later)
//!
//! All three kinds additionally honor an optional `config.expected_http_version`
//! assertion (`"http1"` | `"http2"` | `"http3"`). When set, the negotiated
//! protocol version of the response must match or the heartbeat is forced
//! Down. NOTE: `"http3"` requires reqwest's `http3` feature, which is not
//! enabled in the current build — a monitor configured for `"http3"` will
//! therefore never match a response (responses negotiate HTTP/1.1 or HTTP/2)
//! and will report Down until that feature is compiled in.

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use once_cell::sync::OnceCell;
use rampart_core::monitor::{cert_adjusted_status, CertState};
use rampart_core::proxy::Proxy;
use rampart_core::synthetic::Assertion;
use rampart_core::{Heartbeat, Monitor, MonitorKind, MonitorStatus};
use reqwest::{Client, Method, Version};
use std::collections::BTreeMap;
use std::str::FromStr;
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tracing::warn;

/// User-Agent advertised by the HTTP probe. Built at compile time so it
/// tracks `[workspace.package].version` — bumping the workspace version
/// bumps the UA without a code edit. The trailing `(+url)` follows the
/// long-standing convention for identifying a polite crawler / probe
/// to upstream operators.
const USER_AGENT: &str = concat!(
    "Rampart/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/pen-pal/rampart)"
);

pub struct HttpProbe {
    client: OnceCell<Client>,
}

impl HttpProbe {
    pub fn new() -> Self {
        Self {
            client: OnceCell::new(),
        }
    }

    fn client(&self) -> &Client {
        self.client.get_or_init(|| {
            // Built via the SSRF-guarded resolver so the address actually dialed
            // is vetted at connect time — closes the DNS-rebinding/TOCTOU window
            // the pre-flight resolve_guarded check alone would leave open.
            crate::ssrf::guarded_client_builder()
                .user_agent(USER_AGENT)
                .redirect(reqwest::redirect::Policy::none()) // honored per-monitor below
                .pool_idle_timeout(Duration::from_secs(60))
                .tcp_keepalive(Duration::from_secs(30))
                .build()
                .expect("reqwest client should build with default features")
        })
    }

    /// Variant of `run` that routes through a proxy. We don't pool these
    /// (one client per request) — proxies are typically used for a small
    /// number of monitors, and the pool savings don't justify maintaining
    /// a keyed cache + invalidation when the proxy row changes.
    pub async fn run_with_proxy(&self, monitor: &Monitor, proxy: &Proxy) -> Heartbeat {
        let started = Instant::now();
        let ts = OffsetDateTime::now_utc();

        let client = match build_proxy_client(proxy) {
            Ok(c) => c,
            Err(msg) => return err(monitor, ts, started, &msg),
        };
        self.execute(monitor, &client, started, ts).await
    }
}

fn build_proxy_client(proxy: &Proxy) -> Result<Client, String> {
    let url = format!("{}://{}:{}", proxy.protocol, proxy.host, proxy.port);
    let mut p = reqwest::Proxy::all(&url).map_err(|e| format!("proxy url: {e}"))?;
    if let (Some(u), Some(pw)) = (proxy.username.as_deref(), proxy.password.as_deref()) {
        p = p.basic_auth(u, pw);
    } else if let Some(u) = proxy.username.as_deref() {
        p = p.basic_auth(u, "");
    }
    // NB: not built via the guarded resolver — the client connects to the
    // operator-configured proxy host (trusted infra, often internal), and the
    // proxy performs target resolution out of our reach. The target itself is
    // still SSRF-checked pre-flight in `execute` before the request is issued.
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .redirect(reqwest::redirect::Policy::none())
        .proxy(p)
        .build()
        .map_err(|e| format!("proxy client: {e}"))
}

impl Default for HttpProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Probe for HttpProbe {
    async fn run(&self, monitor: &Monitor) -> Heartbeat {
        let started = Instant::now();
        let ts = OffsetDateTime::now_utc();
        self.execute(monitor, self.client(), started, ts).await
    }
}

impl HttpProbe {
    async fn execute(
        &self,
        monitor: &Monitor,
        client: &Client,
        started: Instant,
        ts: OffsetDateTime,
    ) -> Heartbeat {
        let url = match &monitor.url {
            Some(u) => u.clone(),
            None => return err(monitor, ts, started, "monitor missing url"),
        };

        // SSRF guard: resolve the target and refuse loopback/link-local/cloud
        // metadata (always) + private ranges (when RAMPART_SSRF_BLOCK_PRIVATE
        // is set) before issuing the request.
        if let Ok(parsed) = reqwest::Url::parse(&url) {
            if let Some(host) = parsed.host_str() {
                let port = parsed.port_or_known_default().unwrap_or(80);
                if let Err(blocked) =
                    crate::ssrf::resolve_guarded(host, port, crate::ssrf::block_private_enabled())
                        .await
                {
                    return err(monitor, ts, started, &blocked.to_string());
                }
            }
        }

        let method = Method::from_str(&monitor.http_method).unwrap_or(Method::GET);
        let timeout = Duration::from_secs(monitor.timeout_seconds as u64);

        let mut req = client.request(method, &url).timeout(timeout);

        // Custom headers stored as a JSON object.
        if let Some(serde_json::Value::Object(headers)) = &monitor.http_headers {
            for (k, v) in headers {
                if let Some(s) = v.as_str() {
                    req = req.header(k, s);
                }
            }
        }
        if let Some(body) = &monitor.http_body {
            req = req.body(body.clone());
        }

        let outcome = req.send().await;
        let elapsed = started.elapsed();

        match outcome {
            Ok(resp) => {
                let status_code = resp.status().as_u16() as i32;
                let status_matches = monitor.accepted_statuses.contains(&status_code);

                // Optional negotiated-HTTP-version assertion. Computed before
                // the body is consumed (Response::version() borrows resp).
                let version_check = http_version_matches(resp.version(), &monitor.config);

                // Optional response assertions (status / header / json / body),
                // the same engine synthetics use. Header map is captured here,
                // before the body consumes `resp`.
                let assertions: Vec<Assertion> = monitor
                    .config
                    .get("assertions")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                let resp_headers: BTreeMap<String, String> = if assertions.is_empty() {
                    BTreeMap::new()
                } else {
                    resp.headers()
                        .iter()
                        .filter_map(|(k, v)| {
                            v.to_str()
                                .ok()
                                .map(|val| (k.as_str().to_ascii_lowercase(), val.to_string()))
                        })
                        .collect()
                };

                // Keyword/json_query need the body; so do any assertions.
                let needs_body =
                    matches!(monitor.kind, MonitorKind::Keyword | MonitorKind::JsonQuery)
                        || !assertions.is_empty();
                let body_text = if needs_body {
                    match resp.text().await {
                        Ok(b) => Some(b.chars().take(524_288).collect::<String>()),
                        Err(e) => {
                            warn!(monitor = %monitor.id, error = %e, "body read failed");
                            None
                        }
                    }
                } else {
                    None
                };

                let body_ok = match monitor.kind {
                    MonitorKind::Http => true,
                    MonitorKind::Keyword => match (
                        body_text.as_deref(),
                        monitor.config.get("keyword").and_then(|v| v.as_str()),
                    ) {
                        (Some(b), Some(k)) => b.contains(k),
                        _ => false,
                    },
                    MonitorKind::JsonQuery => {
                        // Minimal scaffold: dotted path lookup + equality check.
                        // Real JSONPath comes later. Format:
                        //   { "json_path": "data.user.active", "expected_value": true }
                        match body_text.as_deref() {
                            Some(b) => json_path_matches(b, &monitor.config),
                            None => false,
                        }
                    }
                    _ => true,
                };

                let version_ok = version_check.is_ok();

                // Assertions: first failure (if any) becomes the down reason.
                let assert_fail = if assertions.is_empty() {
                    None
                } else {
                    crate::synthetic::evaluate_assertions(
                        status_code,
                        &resp_headers,
                        body_text.as_deref().unwrap_or(""),
                        &assertions,
                    )
                };

                // Raw pass/fail. `upside_down` inversion is applied centrally in
                // `Probes::run` (so every kind honours it), not per-probe here.
                let ok = status_matches && body_ok && version_ok && assert_fail.is_none();

                let http_status = if ok {
                    MonitorStatus::Up
                } else {
                    MonitorStatus::Down
                };
                let http_msg = if ok {
                    None
                } else if let Err(version_msg) = &version_check {
                    // A version mismatch is the most specific failure —
                    // surface its clear message ("expected HTTP/2, got
                    // HTTP/1.1") rather than the generic match summary.
                    Some(version_msg.clone())
                } else if let Some(reason) = &assert_fail {
                    // A failed assertion is specific — surface its reason.
                    Some(format!("assertion failed: {reason}"))
                } else {
                    Some(format!(
                        "status_match={status_matches} body_match={body_ok}"
                    ))
                };

                // Opt-in TLS cert-expiry checking. When enabled on an https
                // http-family monitor, inspect the leaf cert and possibly
                // downgrade the status: expired/invalid (and not ignore_tls)
                // → Down; near-expiry → Warn (Up only). A hard HTTP failure
                // already-Down stays Down. When check_cert is false this is
                // a no-op and behaviour is identical to today.
                let (status, msg) = apply_cert_check(monitor, &url, http_status, http_msg).await;

                Heartbeat {
                    monitor_id: monitor.id,
                    ts,
                    status,
                    latency_ms: Some(ms_i32(elapsed)),
                    status_code: Some(status_code),
                    msg,
                    retries: 0,
                    important: false,
                }
            }
            Err(e) if e.is_timeout() => Heartbeat {
                monitor_id: monitor.id,
                ts,
                status: MonitorStatus::Down,
                latency_ms: Some(ms_i32(elapsed)),
                status_code: None,
                msg: Some("request timed out".into()),
                retries: 0,
                important: false,
            },
            Err(e) => Heartbeat {
                monitor_id: monitor.id,
                ts,
                status: MonitorStatus::Down,
                latency_ms: Some(ms_i32(elapsed)),
                status_code: None,
                msg: Some(e.to_string()),
                retries: 0,
                important: false,
            },
        }
    }
}

fn err(monitor: &Monitor, ts: OffsetDateTime, started: Instant, msg: &str) -> Heartbeat {
    Heartbeat {
        monitor_id: monitor.id,
        ts,
        status: MonitorStatus::Down,
        latency_ms: Some(ms_i32(started.elapsed())),
        status_code: None,
        msg: Some(msg.into()),
        retries: 0,
        important: false,
    }
}

/// Opt-in TLS cert-expiry evaluation layered on top of the HTTP check.
///
/// Returns the (possibly adjusted) status + message. A no-op — returning
/// the inputs unchanged — unless ALL of these hold:
///   * `monitor.check_cert` is true (operator opted in), and
///   * the kind is an http-family kind (Http/Keyword/JsonQuery), and
///   * the URL is `https://`.
///
/// When active it re-uses the same `tls::fetch_cert` the scheduler uses to
/// stash the snapshot, derives a [`CertState`], and asks
/// [`cert_adjusted_status`] for the final status. Down/Warn carry a clear
/// cert message; an unchanged status keeps the original HTTP message.
async fn apply_cert_check(
    monitor: &Monitor,
    url: &str,
    http_status: MonitorStatus,
    http_msg: Option<String>,
) -> (MonitorStatus, Option<String>) {
    let is_http_family = matches!(
        monitor.kind,
        MonitorKind::Http | MonitorKind::Keyword | MonitorKind::JsonQuery
    );
    if !monitor.check_cert || !is_http_family || !url.starts_with("https://") {
        return (http_status, http_msg);
    }
    let Some((host, port)) = parse_https(url) else {
        return (http_status, http_msg);
    };

    let to = Duration::from_secs(monitor.timeout_seconds.max(1) as u64);
    let cert = match crate::tls::fetch_cert(&host, port, to).await {
        // fetch_cert verifies against the Mozilla roots, so a handshake
        // success means a currently-valid chain; days_left is the headroom.
        // (A negative value can only appear in the rare clock-skew window
        // where not_after just passed but the handshake still completed —
        // treat that as Expired for safety.)
        Ok(snap) if snap.days_left >= 0 => CertState::Valid {
            days_left: snap.days_left,
        },
        Ok(snap) => CertState::Expired {
            days_ago: -snap.days_left,
        },
        // Handshake / parse failure (expired, self-signed, hostname
        // mismatch, untrusted CA, …) — an invalid cert.
        Err(reason) => CertState::Invalid { reason },
    };

    let status = cert_adjusted_status(
        http_status,
        monitor.check_cert,
        monitor.ignore_tls,
        monitor.cert_expiry_days,
        &cert,
    );

    // Only rewrite the message when the cert actually changed the status,
    // so a still-Up / still-Down check keeps its original message.
    let msg = if status == http_status {
        http_msg
    } else {
        Some(cert_msg(&cert, monitor.cert_expiry_days))
    };
    (status, msg)
}

/// Human-readable reason describing why a cert moved the status.
fn cert_msg(cert: &CertState, threshold: i32) -> String {
    match cert {
        CertState::Valid { days_left } => {
            format!("TLS cert expires in {days_left}d (<= {threshold}d threshold)")
        }
        CertState::Expired { days_ago } => format!("TLS cert expired {days_ago}d ago"),
        CertState::Invalid { reason } => format!("TLS cert invalid: {reason}"),
    }
}

/// Split an `https://host[:port][/path]` URL into (host, port). Mirrors the
/// scheduler's helper; defaults to 443 when no explicit port is present.
fn parse_https(url: &str) -> Option<(String, u16)> {
    let rest = url.strip_prefix("https://")?;
    let host_part = rest.split('/').next()?;
    let (host, port) = match host_part.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse().ok()?),
        None => (host_part.to_string(), 443u16),
    };
    if host.is_empty() {
        return None;
    }
    Some((host, port))
}

/// Minimal JSONPath: dotted path traversal + equality compare.
/// `{"json_path":"data.user.id","expected_value":42}` matches when the
/// path resolves to a value equal to expected_value. Full JSONPath
/// (filters, wildcards) is a follow-up.
fn json_path_matches(body: &str, config: &serde_json::Value) -> bool {
    let parsed: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let path = config
        .get("json_path")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let expected = config
        .get("expected_value")
        .unwrap_or(&serde_json::Value::Null);

    let mut node = &parsed;
    for segment in path.split('.').filter(|s| !s.is_empty()) {
        node = match node.get(segment) {
            Some(n) => n,
            None => return false,
        };
    }
    node == expected
}

/// Human-readable label for a negotiated HTTP version, used in failure
/// messages (e.g. "expected HTTP/2, got HTTP/1.1").
fn version_label(v: Version) -> &'static str {
    match v {
        Version::HTTP_09 => "HTTP/0.9",
        Version::HTTP_10 => "HTTP/1.0",
        Version::HTTP_11 => "HTTP/1.1",
        Version::HTTP_2 => "HTTP/2",
        Version::HTTP_3 => "HTTP/3",
        _ => "unknown",
    }
}

/// Map a config token (`"http1"` | `"http2"` | `"http3"`) to a canonical
/// display label plus the set of reqwest `Version`s it accepts. `http1`
/// accepts any 1.x version (0.9/1.0/1.1) since operators asserting "HTTP/1"
/// generally care about the major line, not the minor revision.
fn expected_versions(token: &str) -> Option<(&'static str, &'static [Version])> {
    match token.trim().to_ascii_lowercase().as_str() {
        "http1" => Some((
            "HTTP/1.1",
            &[Version::HTTP_09, Version::HTTP_10, Version::HTTP_11],
        )),
        "http2" => Some(("HTTP/2", &[Version::HTTP_2])),
        "http3" => Some(("HTTP/3", &[Version::HTTP_3])),
        _ => None,
    }
}

/// Check the negotiated response version against `config.expected_http_version`.
///
/// - `Ok(())` when no assertion is configured, when the token is unknown
///   (treated as "no assertion" — invalid config should not silently fail a
///   monitor), or when the negotiated version satisfies the assertion.
/// - `Err(msg)` with a clear "expected X, got Y" message on mismatch.
fn http_version_matches(actual: Version, config: &serde_json::Value) -> Result<(), String> {
    let token = match config.get("expected_http_version").and_then(|v| v.as_str()) {
        Some(t) if !t.trim().is_empty() => t,
        _ => return Ok(()), // unset → no assertion
    };

    let (want, accepted) = match expected_versions(token) {
        Some(a) => a,
        None => return Ok(()), // unknown token → don't penalize the monitor
    };

    if accepted.contains(&actual) {
        Ok(())
    } else {
        Err(format!("expected {want}, got {}", version_label(actual)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn http_version_unset_always_matches() {
        assert!(http_version_matches(Version::HTTP_11, &json!({})).is_ok());
        assert!(http_version_matches(Version::HTTP_2, &serde_json::Value::Null).is_ok());
    }

    #[test]
    fn http_version_empty_string_treated_as_unset() {
        let cfg = json!({"expected_http_version": ""});
        assert!(http_version_matches(Version::HTTP_11, &cfg).is_ok());
        let cfg = json!({"expected_http_version": "   "});
        assert!(http_version_matches(Version::HTTP_2, &cfg).is_ok());
    }

    #[test]
    fn http_version_http2_matches_http2() {
        let cfg = json!({"expected_http_version": "http2"});
        assert!(http_version_matches(Version::HTTP_2, &cfg).is_ok());
    }

    #[test]
    fn http_version_http2_rejects_http11() {
        let cfg = json!({"expected_http_version": "http2"});
        let err = http_version_matches(Version::HTTP_11, &cfg).unwrap_err();
        assert_eq!(err, "expected HTTP/2, got HTTP/1.1");
    }

    #[test]
    fn http_version_http1_accepts_any_1x() {
        let cfg = json!({"expected_http_version": "http1"});
        assert!(http_version_matches(Version::HTTP_11, &cfg).is_ok());
        assert!(http_version_matches(Version::HTTP_10, &cfg).is_ok());
        assert!(http_version_matches(Version::HTTP_09, &cfg).is_ok());
    }

    #[test]
    fn http_version_http1_rejects_http2() {
        let cfg = json!({"expected_http_version": "http1"});
        let err = http_version_matches(Version::HTTP_2, &cfg).unwrap_err();
        assert_eq!(err, "expected HTTP/1.1, got HTTP/2");
    }

    #[test]
    fn http_version_token_is_case_insensitive() {
        let cfg = json!({"expected_http_version": "HTTP2"});
        assert!(http_version_matches(Version::HTTP_2, &cfg).is_ok());
    }

    #[test]
    fn http_version_unknown_token_does_not_fail() {
        // Garbage config should not silently take a monitor down.
        let cfg = json!({"expected_http_version": "http9000"});
        assert!(http_version_matches(Version::HTTP_11, &cfg).is_ok());
    }

    #[test]
    fn http_version_http3_rejects_http2_under_current_build() {
        // http3 is documented as requiring the reqwest http3 feature; with
        // the current build a response can only be HTTP/1.1 or HTTP/2, so a
        // http3 assertion fails. This locks in that documented behavior.
        let cfg = json!({"expected_http_version": "http3"});
        let err = http_version_matches(Version::HTTP_2, &cfg).unwrap_err();
        assert_eq!(err, "expected HTTP/3, got HTTP/2");
    }

    #[test]
    fn json_path_matches_top_level_value() {
        let body = r#"{"status":"ok"}"#;
        let cfg = json!({"json_path": "status", "expected_value": "ok"});
        assert!(json_path_matches(body, &cfg));
    }

    #[test]
    fn json_path_matches_nested_value() {
        let body = r#"{"data":{"user":{"id":42,"active":true}}}"#;
        let cfg = json!({"json_path": "data.user.id", "expected_value": 42});
        assert!(json_path_matches(body, &cfg));

        let cfg2 = json!({"json_path": "data.user.active", "expected_value": true});
        assert!(json_path_matches(body, &cfg2));
    }

    #[test]
    fn json_path_returns_false_on_wrong_value() {
        let body = r#"{"status":"degraded"}"#;
        let cfg = json!({"json_path": "status", "expected_value": "ok"});
        assert!(!json_path_matches(body, &cfg));
    }

    #[test]
    fn json_path_returns_false_on_missing_segment() {
        let body = r#"{"status":"ok"}"#;
        let cfg = json!({"json_path": "data.user.id", "expected_value": 42});
        assert!(!json_path_matches(body, &cfg));
    }

    #[test]
    fn json_path_returns_false_on_invalid_json() {
        let cfg = json!({"json_path": "x", "expected_value": "y"});
        assert!(!json_path_matches("not json at all", &cfg));
    }

    #[test]
    fn json_path_ignores_leading_or_repeated_dots() {
        // "..data..user..id" should still walk data → user → id.
        let body = r#"{"data":{"user":{"id":1}}}"#;
        let cfg = json!({"json_path": "..data..user..id", "expected_value": 1});
        assert!(json_path_matches(body, &cfg));
    }

    #[test]
    fn json_path_can_compare_null() {
        let body = r#"{"v":null}"#;
        let cfg = json!({"json_path": "v", "expected_value": null});
        assert!(json_path_matches(body, &cfg));
    }

    #[test]
    fn parse_https_defaults_and_explicit_port() {
        assert_eq!(
            parse_https("https://example.com/health"),
            Some(("example.com".into(), 443))
        );
        assert_eq!(
            parse_https("https://example.com:8443/x"),
            Some(("example.com".into(), 8443))
        );
        // plain http and empty host are not parsed for cert checking.
        assert_eq!(parse_https("http://example.com"), None);
        assert_eq!(parse_https("https:///nohost"), None);
    }

    #[test]
    fn cert_msg_renders_each_state() {
        assert_eq!(
            cert_msg(&CertState::Valid { days_left: 7 }, 14),
            "TLS cert expires in 7d (<= 14d threshold)"
        );
        assert_eq!(
            cert_msg(&CertState::Expired { days_ago: 3 }, 14),
            "TLS cert expired 3d ago"
        );
        assert_eq!(
            cert_msg(
                &CertState::Invalid {
                    reason: "handshake: bad cert".into()
                },
                14
            ),
            "TLS cert invalid: handshake: bad cert"
        );
    }
}
