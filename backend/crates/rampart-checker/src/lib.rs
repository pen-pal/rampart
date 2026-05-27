// Test-module-in-the-middle-of-the-file is intentional: keeps unit
// tests next to the small helper functions they cover.
#![allow(clippy::items_after_test_module)]

//! Probe runners.
//!
//! Every monitor kind implements [`Probe`]. The scheduler picks the
//! right probe for each monitor, runs it on whatever interval is
//! configured, and writes the resulting [`Heartbeat`] rows.
//!
//! This crate runs probes. It does no scheduling, no persistence, no
//! alerting — those live elsewhere.

pub mod http;
pub mod tcp;

use async_trait::async_trait;
use rampart_core::{Heartbeat, Monitor, MonitorId, MonitorKind, MonitorStatus};
use std::time::Duration;
use time::OffsetDateTime;

/// Anything that can be probed.
///
/// Implementations should be cheap to construct so we can spin one up
/// per task without thinking about it. Expensive state (HTTP client
/// pools, DNS resolvers) belongs in a shared [`Probes`] handle.
#[async_trait]
pub trait Probe: Send + Sync {
    /// Run one check. Always returns — failures become Heartbeat rows
    /// with a non-Up status, never panics or returns Err.
    async fn run(&self, monitor: &Monitor) -> Heartbeat;
}

/// Bundle of all configured probes. Shares HTTP clients across calls.
pub struct Probes {
    http: http::HttpProbe,
    tcp: tcp::TcpProbe,
}

impl Probes {
    pub fn new() -> Self {
        Self {
            http: http::HttpProbe::new(),
            tcp: tcp::TcpProbe::new(),
        }
    }

    /// Dispatch to the right probe based on monitor kind. Returns a
    /// Heartbeat with status=Down + descriptive msg for kinds not yet
    /// wired up, so the scheduler doesn't need to know which probes
    /// exist.
    pub async fn run(&self, monitor: &Monitor) -> Heartbeat {
        match monitor.kind {
            MonitorKind::Http | MonitorKind::Keyword | MonitorKind::JsonQuery => {
                self.http.run(monitor).await
            }
            MonitorKind::Tcp => self.tcp.run(monitor).await,
            unsupported => unsupported_kind(monitor.id, unsupported),
        }
    }
}

impl Default for Probes {
    fn default() -> Self {
        Self::new()
    }
}

fn unsupported_kind(monitor_id: MonitorId, kind: MonitorKind) -> Heartbeat {
    Heartbeat {
        monitor_id,
        ts: OffsetDateTime::now_utc(),
        status: MonitorStatus::Down,
        latency_ms: None,
        status_code: None,
        msg: Some(format!("probe for {kind:?} not yet implemented")),
        retries: 0,
        important: false,
    }
}

/// Saturating ms conversion that fits in i32.
pub(crate) fn ms_i32(d: Duration) -> i32 {
    d.as_millis().min(i32::MAX as u128) as i32
}
