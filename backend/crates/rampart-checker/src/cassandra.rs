//! Cassandra / ScyllaDB probe.
//!
//! Opens a CQL session to the listed node, runs `SELECT release_version
//! FROM system.local` so the probe exercises both the protocol-level
//! handshake (STARTUP → SUPPORTED → READY) AND a real query path. Up
//! when the query returns a row; Down on connect / auth / query error.
//!
//! `system.local` is guaranteed to exist on every Cassandra-compatible
//! node — it's the per-node bootstrap table — so the query works
//! against both Cassandra and ScyllaDB out of the box, regardless of
//! how the operator has carved up keyspaces.
//!
//! Config (all optional under `monitor.config`):
//! ```json
//! {
//!   "username": "rampart",   // PLAIN auth; omitted = no auth
//!   "password": "..."
//! }
//! ```
//!
//! Hostname + port come off the monitor row; default port 9042 is
//! applied at the call site if the wizard preset took effect.
//! `monitor.timeout_seconds` caps the whole session-plus-query window.
//! Plaintext only today — `cassandras://`-style TLS is a follow-up
//! that would wire our existing rustls ClientConfig through
//! `scylla::SessionConfig` extensions.

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use scylla::client::session_builder::SessionBuilder;
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tokio::time::timeout;

pub struct CassandraProbe;

impl CassandraProbe {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CassandraProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Probe for CassandraProbe {
    async fn run(&self, monitor: &Monitor) -> Heartbeat {
        let started = Instant::now();
        let ts = OffsetDateTime::now_utc();
        let to = Duration::from_secs(monitor.timeout_seconds as u64);

        let host = match monitor.hostname.as_deref() {
            Some(h) if !h.is_empty() => h,
            _ => {
                return down(
                    monitor,
                    ts,
                    started,
                    "cassandra monitor requires hostname (plus optional port; default 9042)",
                )
            }
        };
        let port = monitor.port.unwrap_or(9042) as u16;
        let node = format!("{host}:{port}");

        let mut builder = SessionBuilder::new().known_node(&node);

        if let (Some(u), Some(p)) = (
            monitor.config.get("username").and_then(|v| v.as_str()),
            monitor.config.get("password").and_then(|v| v.as_str()),
        ) {
            builder = builder.user(u, p);
        }

        let session = match timeout(to, builder.build()).await {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => return down(monitor, ts, started, &format!("session: {e}")),
            Err(_) => return down(monitor, ts, started, "session timed out"),
        };

        let query_fut = session.query_unpaged("SELECT release_version FROM system.local", ());
        match timeout(to, query_fut).await {
            Ok(Ok(_)) => Heartbeat {
                monitor_id: monitor.id,
                ts,
                status: MonitorStatus::Up,
                latency_ms: Some(ms_i32(started.elapsed())),
                status_code: None,
                msg: Some(format!("connected to {node} · system.local queried")),
                retries: 0,
                important: false,
            },
            Ok(Err(e)) => down(monitor, ts, started, &format!("query: {e}")),
            Err(_) => down(monitor, ts, started, "query timed out"),
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
