//! NATS probe.
//!
//! Connects to a NATS server, lets the official `async-nats` client run
//! the INFO / CONNECT / PING handshake, then flushes to confirm a
//! round-trip is healthy. Down if connect fails, the handshake errors,
//! or the flush doesn't return within the monitor timeout.
//!
//! Config (all optional under `monitor.config`):
//! ```json
//! { "name": "rampart-probe" }   // client name advertised to the server;
//!                                // defaults to "rampart-checker".
//! ```
//!
//! `monitor.url` carries the `nats://host:port` target. Plaintext only
//! today — `tls://` is a follow-up that wires the rustls ClientConfig
//! we already use for the TLS probe through `async-nats`'s connector.
//! `monitor.timeout_seconds` caps the whole connect-plus-flush window.

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tokio::time::timeout;

pub struct NatsProbe;

impl NatsProbe {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NatsProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Probe for NatsProbe {
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
                    "nats monitor requires url (e.g. nats://host:4222)",
                )
            }
        };

        let name = monitor
            .config
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("rampart-checker");

        let connect_fut = async_nats::ConnectOptions::new()
            .name(name)
            .connection_timeout(to)
            .connect(url);

        let client = match timeout(to, connect_fut).await {
            Ok(Ok(c)) => c,
            Ok(Err(e)) => return down(monitor, ts, started, &format!("connect: {e}")),
            Err(_) => return down(monitor, ts, started, "connect timed out"),
        };

        // `flush()` ensures the client has actually round-tripped a PING
        // / PONG with the server — a successful `connect` alone is too
        // optimistic on some NATS deployments that defer auth failures
        // until the first publish.
        match timeout(to, client.flush()).await {
            Ok(Ok(())) => Heartbeat {
                monitor_id: monitor.id,
                ts,
                status: MonitorStatus::Up,
                latency_ms: Some(ms_i32(started.elapsed())),
                status_code: None,
                msg: Some(format!("connected as {name}")),
                retries: 0,
                important: false,
            },
            Ok(Err(e)) => down(monitor, ts, started, &format!("flush: {e}")),
            Err(_) => down(monitor, ts, started, "flush timed out"),
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
