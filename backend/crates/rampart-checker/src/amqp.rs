//! AMQP 0-9-1 probe.
//!
//! Connects to a RabbitMQ-compatible broker, completes the AMQP protocol
//! handshake via the `lapin` client, then closes cleanly. The handshake
//! covers the TCP connect, the AMQP version negotiation, the SASL auth
//! (PLAIN by default; credentials from the URL), and the tune / open
//! exchange. A clean close means the broker is reachable AND accepting
//! the credentials AND the requested vhost exists.
//!
//! `monitor.url` carries the target as `amqp://user:pass@host:port/vhost`
//! for plaintext or `amqps://…` for TLS. lapin parses the URL scheme via
//! `amq-protocol-uri`'s `AMQPScheme::{AMQP, AMQPS}` and decides at
//! connect-time whether to wrap the socket in TLS — no per-probe
//! connector wiring is needed. The workspace pins lapin to the
//! `rustls--ring` + `rustls-webpki-roots-certs` feature pair, so the
//! `amqps://` path links rustls against the ring `CryptoProvider`
//! `rampart_api::main` installs at startup and verifies against the
//! Mozilla webpki trust anchors — same crypto stack as the
//! `websocket.rs` `wss://` path. The `rustls` (and `rustls-native-certs`)
//! aliases are deliberately left disabled because their transitive
//! `rustls--aws_lc_rs` would drag `aws-lc-rs` + `cmake` into the build
//! graph (see docs/DEPENDENCIES.md).
//!
//! `monitor.timeout_seconds` caps the whole connect-plus-close window.
//! No probe-side config knobs — everything the broker needs is encoded
//! in the URL.

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use lapin::{Connection, ConnectionProperties};
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tokio::time::timeout;

pub struct AmqpProbe;

impl AmqpProbe {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AmqpProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Probe for AmqpProbe {
    async fn run(&self, monitor: &Monitor) -> Heartbeat {
        let started = Instant::now();
        let ts = OffsetDateTime::now_utc();
        let to = Duration::from_secs(monitor.timeout_seconds as u64);

        let url = match monitor.url.as_deref() {
            Some(u) if !u.is_empty() => u,
            _ => {
                return down(
                    monitor,
                    ts,
                    started,
                    "amqp monitor requires url (e.g. amqp://user:pass@host:5672/vhost \
                     or amqps://… for TLS)",
                )
            }
        };

        // lapin auto-detects the ambient async runtime — tokio in our
        // case — so the default ConnectionProperties suffice. Helper
        // tasks spawned by the client get cleaned up when the
        // Connection drops at end of scope.
        let connect_fut = Connection::connect(url, ConnectionProperties::default());

        let conn = match timeout(to, connect_fut).await {
            Ok(Ok(c)) => c,
            Ok(Err(e)) => return down(monitor, ts, started, &format!("connect: {e}")),
            Err(_) => return down(monitor, ts, started, "connect timed out"),
        };

        // A successful connect already covers handshake + auth + vhost
        // selection. Explicitly close so the broker sees the goodbye
        // frame instead of a half-open socket — matters for brokers
        // that page on un-graceful disconnects.
        match timeout(to, conn.close(200, "Bye")).await {
            Ok(Ok(())) => Heartbeat {
                monitor_id: monitor.id,
                ts,
                status: MonitorStatus::Up,
                latency_ms: Some(ms_i32(started.elapsed())),
                status_code: None,
                msg: Some("handshake OK".into()),
                retries: 0,
                important: false,
            },
            Ok(Err(e)) => down(monitor, ts, started, &format!("close: {e}")),
            Err(_) => down(monitor, ts, started, "close timed out"),
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
    use lapin::uri::{AMQPScheme, AMQPUri};
    use std::str::FromStr;

    /// `amqps://` URIs must parse to `AMQPScheme::AMQPS`. If the
    /// workspace ever drops the `rustls--ring` /
    /// `rustls-webpki-roots-certs` feature pair on `lapin`, the
    /// *runtime* probe would fail with a TLS error on connect, but the
    /// parser itself still recognises the scheme — so this test catches
    /// a refactor that accidentally narrows the accepted URL set
    /// (e.g. an early-reject path), not the missing feature flag.
    #[test]
    fn amqps_uri_parses_with_tls_scheme() {
        let uri = AMQPUri::from_str("amqps://user:pass@broker.example.com:5671/%2f")
            .expect("amqps:// URL should parse into an AMQPUri");
        assert!(matches!(uri.scheme, AMQPScheme::AMQPS));
        assert_eq!(uri.authority.host, "broker.example.com");
        assert_eq!(uri.authority.port, 5671);
    }

    /// Mirror of the above for the plaintext scheme — a future refactor
    /// that drops `amqp://` support in favour of TLS-only would trip a
    /// test rather than silently regress against existing monitors.
    #[test]
    fn amqp_uri_parses_with_plaintext_scheme() {
        let uri = AMQPUri::from_str("amqp://guest:guest@localhost:5672/")
            .expect("amqp:// URL should parse into an AMQPUri");
        assert!(matches!(uri.scheme, AMQPScheme::AMQP));
        assert_eq!(uri.authority.host, "localhost");
        assert_eq!(uri.authority.port, 5672);
    }
}
