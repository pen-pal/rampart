//! mDNS service-discovery probe (RFC 6762).
//!
//! Sends a multicast DNS query for `_services._dns-sd._udp.local` —
//! the DNS-SD meta-service that enumerates all services advertised on
//! a link — to the IPv4 mDNS group at `224.0.0.251:5353`, then counts
//! unicast responses arriving over the monitor timeout window. Up when
//! at least one peer answers; Down when nothing replies inside the
//! window or the socket binding fails.
//!
//! The query packet is hand-rolled rather than pulled from a crate
//! because the wire format is trivial (12-byte header + one question)
//! and going through an mDNS daemon abstraction would require taking
//! a heavier dep on a runtime that runs continuously, which the probe
//! doesn't need.
//!
//! Config (all optional under `monitor.config`):
//! ```json
//! { "service": "_services._dns-sd._udp.local" }
//! ```
//!
//! Setting `service` to a specific advertised name (e.g.
//! `_http._tcp.local`) narrows the probe to that service class —
//! useful for confirming a specific advertiser is on the LAN.

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use std::net::{Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tokio::net::UdpSocket;
use tokio::time::timeout;

const MDNS_GROUP: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
const MDNS_PORT: u16 = 5353;

pub struct MdnsProbe;

impl MdnsProbe {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MdnsProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Probe for MdnsProbe {
    async fn run(&self, monitor: &Monitor) -> Heartbeat {
        let started = Instant::now();
        let ts = OffsetDateTime::now_utc();
        let to = Duration::from_secs(monitor.timeout_seconds as u64);

        let service = monitor
            .config
            .get("service")
            .and_then(|v| v.as_str())
            .unwrap_or("_services._dns-sd._udp.local");

        // Bind ephemeral; OS picks a free UDP port on 0.0.0.0.
        let sock = match UdpSocket::bind("0.0.0.0:0").await {
            Ok(s) => s,
            Err(e) => return down(monitor, ts, started, &format!("bind: {e}")),
        };
        // Joining the multicast group lets us receive frames the OS
        // would otherwise drop. Bind to any local interface — operators
        // running on a multi-NIC host can pin via OS routing.
        if let Err(e) = sock.join_multicast_v4(MDNS_GROUP, Ipv4Addr::UNSPECIFIED) {
            return down(monitor, ts, started, &format!("join group: {e}"));
        }

        let query = match build_dns_query(service) {
            Ok(q) => q,
            Err(e) => return down(monitor, ts, started, &format!("encode: {e}")),
        };

        if let Err(e) = sock
            .send_to(&query, SocketAddr::new(MDNS_GROUP.into(), MDNS_PORT))
            .await
        {
            return down(monitor, ts, started, &format!("send: {e}"));
        }

        let mut buf = [0u8; 1500];
        match timeout(to, sock.recv_from(&mut buf)).await {
            Ok(Ok((n, from))) => Heartbeat {
                monitor_id: monitor.id,
                ts,
                status: MonitorStatus::Up,
                latency_ms: Some(ms_i32(started.elapsed())),
                status_code: None,
                msg: Some(format!("{n}B from {from}")),
                retries: 0,
                important: false,
            },
            Ok(Err(e)) => down(monitor, ts, started, &format!("recv: {e}")),
            Err(_) => down(monitor, ts, started, "no mDNS responses within timeout"),
        }
    }
}

/// Encode a single-question DNS query for the given service name.
/// Header is the standard 12 bytes (id=0, flags=0, qdcount=1, ancount/
/// nscount/arcount=0). Question is the labels-encoded name, QTYPE=PTR
/// (12), QCLASS=IN (1).
fn build_dns_query(service: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(service.len() + 20);
    // Header
    out.extend_from_slice(&[0, 0]); // transaction id
    out.extend_from_slice(&[0, 0]); // flags (standard query, not RD)
    out.extend_from_slice(&[0, 1]); // qdcount
    out.extend_from_slice(&[0, 0]); // ancount
    out.extend_from_slice(&[0, 0]); // nscount
    out.extend_from_slice(&[0, 0]); // arcount

    // Question: labels-encoded name
    for label in service.trim_end_matches('.').split('.') {
        if label.is_empty() {
            return Err("empty label in service name".into());
        }
        if label.len() > 63 {
            return Err(format!("label too long: {label}"));
        }
        out.push(label.len() as u8);
        out.extend_from_slice(label.as_bytes());
    }
    out.push(0); // root label terminator
    out.extend_from_slice(&[0, 12]); // QTYPE = PTR
    out.extend_from_slice(&[0, 1]); // QCLASS = IN
    Ok(out)
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
    use super::build_dns_query;

    #[test]
    fn encodes_root_terminated_labels() {
        let q = build_dns_query("_http._tcp.local").unwrap();
        // Header: 12 bytes
        assert_eq!(&q[..2], &[0, 0]); // id
        assert_eq!(&q[4..6], &[0, 1]); // qdcount
                                       // Question starts at offset 12:
                                       //   1 label  "_http"  (5)
                                       //   1 label  "_tcp"   (4)
                                       //   1 label  "local"  (5)
                                       //   root terminator (0)
        assert_eq!(q[12], 5);
        assert_eq!(&q[13..18], b"_http");
        assert_eq!(q[18], 4);
        assert_eq!(&q[19..23], b"_tcp");
        assert_eq!(q[23], 5);
        assert_eq!(&q[24..29], b"local");
        assert_eq!(q[29], 0);
        // QTYPE=12 (PTR), QCLASS=1 (IN)
        assert_eq!(&q[30..32], &[0, 12]);
        assert_eq!(&q[32..34], &[0, 1]);
    }

    #[test]
    fn rejects_empty_label() {
        assert!(build_dns_query("foo..bar").is_err());
    }

    #[test]
    fn rejects_oversized_label() {
        let big = "a".repeat(64);
        assert!(build_dns_query(&big).is_err());
    }
}
