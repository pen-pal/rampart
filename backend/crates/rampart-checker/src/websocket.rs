//! WebSocket probe (RFC 6455) — connect, complete the HTTP upgrade,
//! optionally read one text frame and assert it contains a substring.
//!
//! Config (all optional under `monitor.config`):
//! ```json
//! { "expect": "ready" }   // substring required in the first text frame
//! ```
//!
//! `monitor.url` carries the `ws://host:port/path` or `wss://host:port/path`
//! target. `wss://` URIs negotiate TLS through `tokio-tungstenite`'s
//! built-in `MaybeTlsStream`: the workspace enables the
//! `rustls-tls-webpki-roots` feature, so the connector switches on URL
//! scheme — `ws://` stays plain TCP, `wss://` wraps the socket in a
//! rustls `TlsStream` seeded with the `webpki-roots` trust anchors. The
//! crypto provider is the workspace-shared `ring` `CryptoProvider`
//! installed at startup in `rampart_api::main` via
//! `rustls::crypto::ring::default_provider().install_default()`, which
//! keeps the entire workspace on a single C-free crypto stack (see
//! docs/DEPENDENCIES.md). No per-probe `Connector` wiring is needed.
//!
//! `monitor.timeout_seconds` caps the whole probe (connect + TLS
//! handshake + WebSocket handshake + optional read). The probe
//! explicitly closes the connection on the way out so the server
//! doesn't see a half-open socket.

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;

pub struct WebsocketProbe;

impl WebsocketProbe {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WebsocketProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Probe for WebsocketProbe {
    async fn run(&self, monitor: &Monitor) -> Heartbeat {
        let started = Instant::now();
        let ts = OffsetDateTime::now_utc();
        let to = Duration::from_secs(monitor.timeout_seconds as u64);

        let Some(url) = monitor.url.as_deref() else {
            return down(monitor, ts, started, "websocket monitor requires a url");
        };
        if !(url.starts_with("ws://") || url.starts_with("wss://")) {
            return down(monitor, ts, started, "url must start with ws:// or wss://");
        }
        let expect = monitor
            .config
            .get("expect")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Handshake — this is the bulk of what we're testing.
        let stream = match timeout(to, tokio_tungstenite::connect_async(url)).await {
            Ok(Ok((s, _resp))) => s,
            Ok(Err(e)) => return down(monitor, ts, started, &format!("handshake failed: {e}")),
            Err(_) => return down(monitor, ts, started, "handshake timed out"),
        };
        let (mut write, mut read) = stream.split();

        // If the user asked for an `expect` substring, read one frame.
        // Otherwise the handshake itself is the assertion.
        let msg = if expect.is_some() {
            match timeout(to, read.next()).await {
                Ok(Some(Ok(Message::Text(t)))) => Some(t.to_string()),
                Ok(Some(Ok(Message::Binary(b)))) => Some(String::from_utf8_lossy(&b).into_owned()),
                Ok(Some(Ok(other))) => Some(format!("non-text frame: {other:?}")),
                Ok(Some(Err(e))) => {
                    return down(monitor, ts, started, &format!("read failed: {e}"));
                }
                Ok(None) => {
                    return down(
                        monitor,
                        ts,
                        started,
                        "server closed without sending a frame",
                    );
                }
                Err(_) => return down(monitor, ts, started, "read timed out"),
            }
        } else {
            None
        };

        // Politely close the socket. Errors here are non-fatal — the
        // probe already validated what it cares about.
        let _ = write.send(Message::Close(None)).await;

        if let (Some(needle), Some(body)) = (expect.as_deref(), msg.as_deref()) {
            if !body.contains(needle) {
                return down(
                    monitor,
                    ts,
                    started,
                    &format!("expected '{needle}' not found in first frame"),
                );
            }
        }

        Heartbeat {
            monitor_id: monitor.id,
            ts,
            status: MonitorStatus::Up,
            latency_ms: Some(ms_i32(started.elapsed())),
            status_code: None,
            msg: msg.map(|m| {
                if m.len() > 120 {
                    format!("{}…", &m[..120])
                } else {
                    m
                }
            }),
            retries: 0,
            important: false,
        }
    }
}

fn down(monitor: &Monitor, ts: OffsetDateTime, started: Instant, msg: &str) -> Heartbeat {
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

#[cfg(test)]
mod tests {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    /// Sanity check that `wss://` URIs parse through the same
    /// `IntoClientRequest` path that `tokio_tungstenite::connect_async`
    /// uses internally. If the workspace ever drops the
    /// `rustls-tls-webpki-roots` feature, the *runtime* probe would
    /// fail with `TlsFeatureNotEnabled` — this test only proves URL
    /// parsing + scheme acceptance, no network I/O.
    #[test]
    fn wss_url_parses_as_client_request() {
        let req = "wss://echo.websocket.org/"
            .into_client_request()
            .expect("wss:// URL should parse into a client request");
        assert_eq!(req.uri().scheme_str(), Some("wss"));
        assert_eq!(req.uri().host(), Some("echo.websocket.org"));
    }

    /// Mirror of the above for the plaintext scheme, so a future
    /// refactor that accidentally narrows the accepted scheme set
    /// trips a test rather than a production probe.
    #[test]
    fn ws_url_parses_as_client_request() {
        let req = "ws://example.com:8080/socket"
            .into_client_request()
            .expect("ws:// URL should parse into a client request");
        assert_eq!(req.uri().scheme_str(), Some("ws"));
        assert_eq!(req.uri().host(), Some("example.com"));
        assert_eq!(req.uri().port_u16(), Some(8080));
    }
}
