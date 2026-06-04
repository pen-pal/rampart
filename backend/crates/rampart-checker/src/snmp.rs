//! SNMP GET probe.
//!
//! Issues a single SNMP v2c GET against a UDP-reachable agent (port
//! 161 by default) and asserts the agent replied with a non-empty
//! varbind. Same wire format as SNMPv1 for the GET PDU — the probe
//! works against both v1- and v2c-only agents.
//!
//! Config (all optional under `monitor.config`):
//! ```json
//! {
//!   "community": "public",                  // SNMP community string
//!   "oid":       "1.3.6.1.2.1.1.1.0"        // OID to query (default
//!                                            // sysDescr.0 — every
//!                                            // SNMP agent populates
//!                                            // this with vendor +
//!                                            // version text)
//! }
//! ```
//!
//! Hostname + port come off the monitor row. `monitor.timeout_seconds`
//! caps the request — the wire library accepts a `Duration` directly so
//! no manual timeout race is needed.

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use csnmp::{ObjectIdentifier, Snmp2cClient};
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use std::net::{SocketAddr, ToSocketAddrs};
use std::str::FromStr;
use std::time::{Duration, Instant};
use time::OffsetDateTime;

pub struct SnmpProbe;

impl SnmpProbe {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SnmpProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Probe for SnmpProbe {
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
                    "snmp monitor requires hostname (plus optional port; default 161)",
                )
            }
        };
        let port = monitor.port.unwrap_or(161) as u16;

        // csnmp wants a `SocketAddr`, not a hostname. Resolve via the
        // blocking std resolver wrapped in `tokio::task::spawn_blocking`
        // so we don't tie up the runtime. `(host, port)` to_socket_addrs
        // returns multiple entries for hosts that resolve to both v4 +
        // v6 — the SNMP probe just picks the first; operators wanting
        // to target a specific family can use a v4/v6 literal.
        let host_owned = host.to_string();
        let resolved = match tokio::task::spawn_blocking(move || {
            (host_owned.as_str(), port).to_socket_addrs().map(|mut it| it.next())
        })
        .await
        {
            Ok(Ok(Some(a))) => a,
            Ok(Ok(None)) => return down(monitor, ts, started, "resolve: no addresses"),
            Ok(Err(e)) => return down(monitor, ts, started, &format!("resolve: {e}")),
            Err(e) => return down(monitor, ts, started, &format!("resolve task: {e}")),
        };

        let community = monitor
            .config
            .get("community")
            .and_then(|v| v.as_str())
            .unwrap_or("public")
            .to_string();
        let oid_str = monitor
            .config
            .get("oid")
            .and_then(|v| v.as_str())
            .unwrap_or("1.3.6.1.2.1.1.1.0");

        let oid = match ObjectIdentifier::from_str(oid_str) {
            Ok(o) => o,
            Err(e) => return down(monitor, ts, started, &format!("bad oid {oid_str}: {e}")),
        };

        let client = match Snmp2cClient::new(
            resolved as SocketAddr,
            community.into_bytes(),
            None,
            Some(to),
        )
        .await
        {
            Ok(c) => c,
            Err(e) => return down(monitor, ts, started, &format!("client: {e}")),
        };

        match client.get(oid).await {
            Ok(val) => Heartbeat {
                monitor_id: monitor.id,
                ts,
                status: MonitorStatus::Up,
                latency_ms: Some(ms_i32(started.elapsed())),
                status_code: None,
                msg: Some(format!("{oid_str} = {val:?}")),
                retries: 0,
                important: false,
            },
            Err(e) => down(monitor, ts, started, &format!("get {oid_str}: {e}")),
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
