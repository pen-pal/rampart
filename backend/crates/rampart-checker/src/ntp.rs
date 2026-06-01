//! NTP probe — SNTPv4 client request/response.
//!
//! Builds a 48-byte SNTPv4 client packet (li=0, vn=4, mode=3, all other
//! fields zero except a freshly-randomised Transmit Timestamp so we can
//! pair the response with the request), sends it over UDP to host:port
//! (default 123), and asserts the server replies with mode=server (4) or
//! mode=broadcast (5) and a non-zero stratum (stratum=0 means the
//! server is unsynchronised; an answer that arrived but is unsynced is
//! still a soft-fail rather than a connectivity failure, so we warn).
//!
//! Doesn't compute clock offset/jitter — this is a connectivity check,
//! not a discipline source. Time-sync drift monitoring belongs in a
//! separate probe (chrony / ntpq style).

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tokio::net::UdpSocket;
use tokio::time::timeout;

const DEFAULT_PORT: u16 = 123;

pub struct NtpProbe;

impl NtpProbe {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NtpProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Probe for NtpProbe {
    async fn run(&self, monitor: &Monitor) -> Heartbeat {
        let started = Instant::now();
        let ts = OffsetDateTime::now_utc();
        let to = Duration::from_secs(monitor.timeout_seconds as u64);

        let Some(host) = monitor.hostname.as_deref() else {
            return down(monitor, ts, started, "ntp monitor requires a hostname");
        };
        let port = monitor.port.map(|p| p as u16).unwrap_or(DEFAULT_PORT);
        let target = format!("{host}:{port}");

        // SNTPv4 client packet. Byte 0 = LI(0) << 6 | VN(4) << 3 | Mode(3) = 0x23.
        // Bytes 40..48 = Transmit Timestamp; we stamp a non-zero nonce so the
        // server's reply originates from a packet we recognise.
        let mut req = [0u8; 48];
        req[0] = 0x23;
        // Borrow a few low bits of the OffsetDateTime nanos for the nonce.
        let nonce = ts.unix_timestamp_nanos() as u64;
        req[40..48].copy_from_slice(&nonce.to_be_bytes());

        let sock = match UdpSocket::bind("0.0.0.0:0").await {
            Ok(s) => s,
            Err(e) => return down(monitor, ts, started, &format!("bind failed: {e}")),
        };
        if let Err(e) = timeout(to, sock.send_to(&req, &target)).await {
            return down(monitor, ts, started, &format!("send timed out: {e}"));
        }

        let mut buf = [0u8; 48];
        match timeout(to, sock.recv(&mut buf)).await {
            Ok(Ok(n)) if n < 48 => {
                return down(monitor, ts, started, &format!("short reply: {n} bytes"));
            }
            Ok(Ok(_)) => {}
            Ok(Err(e)) => return down(monitor, ts, started, &format!("recv failed: {e}")),
            Err(_) => return down(monitor, ts, started, "no reply (timeout)"),
        }

        // mode lives in bits 0..2 of byte 0. 4 = server, 5 = broadcast.
        let mode = buf[0] & 0x07;
        if mode != 4 && mode != 5 {
            return down(
                monitor,
                ts,
                started,
                &format!("unexpected mode in reply: {mode}"),
            );
        }
        let stratum = buf[1];
        let latency = ms_i32(started.elapsed());
        // Stratum 0 = "kiss-o'-death" or unsynchronised; surface as warn so
        // the alerting path still fires for ops, but the probe didn't fail
        // at the network layer.
        if stratum == 0 {
            return Heartbeat {
                monitor_id: monitor.id,
                ts,
                status: MonitorStatus::Warn,
                latency_ms: Some(latency),
                status_code: None,
                msg: Some("server replied but stratum=0 (kiss-o'-death / unsynced)".into()),
                retries: 0,
                important: false,
            };
        }

        Heartbeat {
            monitor_id: monitor.id,
            ts,
            status: MonitorStatus::Up,
            latency_ms: Some(latency),
            status_code: None,
            msg: Some(format!("stratum={stratum} mode={mode}")),
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
