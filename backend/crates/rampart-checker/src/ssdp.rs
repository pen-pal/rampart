//! SSDP / UPnP service-discovery probe.
//!
//! Sends an HTTP-over-UDP `M-SEARCH * HTTP/1.1` datagram to the SSDP
//! multicast group at `239.255.255.250:1900`. UPnP-aware devices on
//! the LAN reply unicast within `MX` seconds; the probe counts the
//! first response that arrives inside the monitor timeout. Up when at
//! least one peer answers; Down when nothing replies.
//!
//! Config (all optional under `monitor.config`):
//! ```json
//! {
//!   "search_target": "ssdp:all"   // ST header value; "ssdp:all"
//!                                  // matches every UPnP device,
//!                                  // "upnp:rootdevice" narrows to
//!                                  // root devices only, or a
//!                                  // service URI like
//!                                  // "urn:schemas-upnp-org:device:
//!                                  // MediaRenderer:1" targets a
//!                                  // specific device class.
//! }
//! ```
//!
//! No hostname / port — both are fixed by the SSDP spec.

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use std::net::{Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tokio::net::UdpSocket;
use tokio::time::timeout;

const SSDP_GROUP: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);
const SSDP_PORT: u16 = 1900;

pub struct SsdpProbe;

impl SsdpProbe {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SsdpProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Probe for SsdpProbe {
    async fn run(&self, monitor: &Monitor) -> Heartbeat {
        let started = Instant::now();
        let ts = OffsetDateTime::now_utc();
        let to = Duration::from_secs(monitor.timeout_seconds as u64);

        let st = monitor
            .config
            .get("search_target")
            .and_then(|v| v.as_str())
            .unwrap_or("ssdp:all");

        // SSDP's `MX` header is the max wait the responder should
        // randomise their reply over. Cap it at `min(timeout, 5)` —
        // values > 5 are out-of-spec and tend to be ignored.
        let mx = monitor.timeout_seconds.clamp(1, 5);

        // Header values that include CRLF terminate request parsing
        // mid-line, so we own the framing manually rather than via a
        // crate. Trailing CRLF-CRLF closes the request as required by
        // the SSDP spec.
        let req = format!(
            "M-SEARCH * HTTP/1.1\r\n\
             HOST: {SSDP_GROUP}:{SSDP_PORT}\r\n\
             MAN: \"ssdp:discover\"\r\n\
             MX: {mx}\r\n\
             ST: {st}\r\n\
             USER-AGENT: rampart/{} (uptime-monitor)\r\n\
             \r\n",
            env!("CARGO_PKG_VERSION")
        );

        let sock = match UdpSocket::bind("0.0.0.0:0").await {
            Ok(s) => s,
            Err(e) => return down(monitor, ts, started, &format!("bind: {e}")),
        };

        if let Err(e) = sock
            .send_to(req.as_bytes(), SocketAddr::new(SSDP_GROUP.into(), SSDP_PORT))
            .await
        {
            return down(monitor, ts, started, &format!("send: {e}"));
        }

        let mut buf = [0u8; 1500];
        match timeout(to, sock.recv_from(&mut buf)).await {
            Ok(Ok((n, from))) => {
                let preview = preview_status_line(&buf[..n]);
                Heartbeat {
                    monitor_id: monitor.id,
                    ts,
                    status: MonitorStatus::Up,
                    latency_ms: Some(ms_i32(started.elapsed())),
                    status_code: None,
                    msg: Some(format!("{from} · {preview}")),
                    retries: 0,
                    important: false,
                }
            }
            Ok(Err(e)) => down(monitor, ts, started, &format!("recv: {e}")),
            Err(_) => down(monitor, ts, started, "no SSDP responses within timeout"),
        }
    }
}

/// Pull the first line of the SSDP response for the heartbeat msg,
/// truncated to a reasonable length. Cleaner than dumping the raw
/// bytes; gives the operator a hint about which device answered.
fn preview_status_line(payload: &[u8]) -> String {
    let line_end = payload
        .iter()
        .position(|b| *b == b'\r' || *b == b'\n')
        .unwrap_or(payload.len());
    let s = String::from_utf8_lossy(&payload[..line_end]);
    if s.len() > 80 {
        format!("{}…", &s[..80])
    } else {
        s.into_owned()
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
    use super::preview_status_line;

    #[test]
    fn preview_takes_first_line() {
        let payload =
            b"HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=1800\r\nLOCATION: http://x/desc.xml\r\n";
        assert_eq!(preview_status_line(payload), "HTTP/1.1 200 OK");
    }

    #[test]
    fn preview_handles_truncation() {
        let big = vec![b'A'; 200];
        let out = preview_status_line(&big);
        assert!(out.ends_with('…'));
        assert!(out.len() < 100);
    }
}
