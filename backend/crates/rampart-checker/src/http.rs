//! HTTP / HTTPS probe.
//!
//! Handles three monitor kinds with one probe:
//! - Http        — status code matches `accepted_statuses`
//! - Keyword     — `accepted_statuses` AND body contains `config.keyword`
//! - JsonQuery   — `accepted_statuses` AND `config.json_path` returns a
//!   value equal to `config.expected_value` (simplest form — full JSONPath
//!   comes later)

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use once_cell::sync::OnceCell;
use rampart_core::proxy::Proxy;
use rampart_core::{Heartbeat, Monitor, MonitorKind, MonitorStatus};
use reqwest::{Client, ClientBuilder, Method};
use std::str::FromStr;
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tracing::warn;

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
            ClientBuilder::new()
                .user_agent("Rampart/0.1 (+https://github.com/rampart-io/rampart)")
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
    ClientBuilder::new()
        .user_agent("Rampart/0.1 (+https://github.com/rampart-io/rampart)")
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

                // For keyword/json_query monitors we need the body.
                let needs_body =
                    matches!(monitor.kind, MonitorKind::Keyword | MonitorKind::JsonQuery);
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

                // upside_down inverts the pass/fail decision (useful for
                // monitoring services that should be down, like a
                // honeypot or staging instance).
                let raw_ok = status_matches && body_ok;
                let ok = if monitor.upside_down { !raw_ok } else { raw_ok };

                Heartbeat {
                    monitor_id: monitor.id,
                    ts,
                    status: if ok {
                        MonitorStatus::Up
                    } else {
                        MonitorStatus::Down
                    },
                    latency_ms: Some(ms_i32(elapsed)),
                    status_code: Some(status_code),
                    msg: if ok {
                        None
                    } else {
                        Some(format!(
                            "status_match={status_matches} body_match={body_ok}"
                        ))
                    },
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
}
