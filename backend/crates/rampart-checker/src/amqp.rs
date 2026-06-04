//! AMQP 0-9-1 probe.
//!
//! Connects to a RabbitMQ-compatible broker, completes the AMQP protocol
//! handshake via the `lapin` client, then closes cleanly. The handshake
//! covers the TCP connect, the AMQP version negotiation, the SASL auth
//! (PLAIN by default; credentials from the URL), and the tune / open
//! exchange. A clean close means the broker is reachable AND accepting
//! the credentials AND the requested vhost exists.
//!
//! `monitor.url` carries the target as `amqp://user:pass@host:port/vhost`.
//! `amqps://` (TLS) is deferred — the probe relies on lapin's default
//! plain transport today; a future pass will wire our existing rustls
//! ClientConfig through `lapin::Connect::connect_with_config`.
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
                    "amqp monitor requires url (e.g. amqp://user:pass@host:5672/vhost)",
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
