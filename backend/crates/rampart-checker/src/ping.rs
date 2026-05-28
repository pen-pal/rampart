//! ICMP echo probe.
//!
//! Uses [`surge-ping`] which talks raw ICMP via the OS. Two deployment
//! caveats worth knowing:
//!
//! - **Linux**: needs either `CAP_NET_RAW` on the binary, or the kernel's
//!   unprivileged ICMP path enabled via `sysctl net.ipv4.ping_group_range`.
//!   The README documents both setups. If neither is set, you'll get an
//!   `EACCES` and the heartbeat will say so explicitly.
//! - **macOS**: unprivileged ICMP works out of the box.
//! - **Docker**: include `--cap-add=NET_RAW` in `docker compose`.
//!
//! Config keys read from `monitor.config`:
//!
//! ```json
//! { "packet_size": 56 }    // payload size in bytes; default 56 (BSD)
//! ```
//!
//! Picks IPv4 or IPv6 automatically based on the resolved address.

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::time::{Duration, Instant};
use surge_ping::{Client, Config, IcmpPacket, PingIdentifier, PingSequence};
use time::OffsetDateTime;
use tokio::time::timeout;

pub struct PingProbe;

impl PingProbe {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PingProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Probe for PingProbe {
    async fn run(&self, monitor: &Monitor) -> Heartbeat {
        let started = Instant::now();
        let ts = OffsetDateTime::now_utc();
        let to = Duration::from_secs(monitor.timeout_seconds as u64);

        let Some(host) = monitor.hostname.as_deref() else {
            return down(monitor, ts, started, "ping monitor requires a hostname");
        };

        // resolve. surge-ping wants an IpAddr; getaddrinfo gives us one.
        let resolved = match resolve(host).await {
            Ok(ip) => ip,
            Err(e) => return down(monitor, ts, started, &format!("resolve failed: {e}")),
        };

        let packet_size = monitor
            .config
            .get("packet_size")
            .and_then(|v| v.as_u64())
            .unwrap_or(56) as usize;

        // PingIdentifier collides only on the same client+sequence, and
        // since we build a fresh client per probe a static value is fine.
        let ident = PingIdentifier(rand::random::<u16>());
        let payload = vec![0u8; packet_size];

        let config = match resolved {
            IpAddr::V4(_) => Config::default(),
            IpAddr::V6(_) => Config::builder().kind(surge_ping::ICMP::V6).build(),
        };

        let client = match Client::new(&config) {
            Ok(c) => c,
            Err(e) => {
                // The most common cause is missing CAP_NET_RAW. Surface
                // the OS-level error verbatim so the user can grep the
                // README for it without us guessing.
                return down(monitor, ts, started, &format!("icmp socket: {e}"));
            }
        };

        let mut pinger = client.pinger(resolved, ident).await;
        pinger.timeout(to);

        let send = pinger.ping(PingSequence(0), &payload);
        match timeout(to, send).await {
            Ok(Ok((IcmpPacket::V4(_), dur))) | Ok(Ok((IcmpPacket::V6(_), dur))) => Heartbeat {
                monitor_id: monitor.id,
                ts,
                status: MonitorStatus::Up,
                latency_ms: Some(ms_i32(dur)),
                status_code: None,
                msg: Some(format!("{}ms", dur.as_millis())),
                retries: 0,
                important: false,
            },
            Ok(Err(e)) => down(monitor, ts, started, &format!("ping failed: {e}")),
            Err(_) => down(monitor, ts, started, "ping timed out"),
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

/// getaddrinfo without picking a port. Returns the first address — we
/// don't try to be clever about IPv6 vs IPv4 here; whichever the OS
/// orders first wins. Operators who care can pin to an IP literal.
async fn resolve(host: &str) -> Result<IpAddr, String> {
    let host = host.to_string();
    let addrs = tokio::task::spawn_blocking(move || {
        (host.as_str(), 0u16)
            .to_socket_addrs()
            .map(|i| i.collect::<Vec<SocketAddr>>())
    })
    .await
    .map_err(|e| format!("resolver task: {e}"))?
    .map_err(|e| format!("getaddrinfo: {e}"))?;

    addrs
        .into_iter()
        .next()
        .map(|s| s.ip())
        .ok_or_else(|| "no addresses returned".into())
}
