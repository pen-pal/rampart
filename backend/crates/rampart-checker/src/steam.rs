//! Steam game-server probe — A2S_INFO query.
//!
//! Valve's lightweight query protocol over UDP. We send the canonical
//! `A2S_INFO` packet ("\xFF\xFF\xFF\xFFTSource Engine Query\0") and
//! treat any well-formed reply as Up. Servers that demand the
//! 4-byte challenge handshake reply with header `0x41` first — we
//! repeat the request with the supplied challenge and accept the
//! second response.
//!
//! Docs: https://developer.valvesoftware.com/wiki/Server_queries

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tokio::net::UdpSocket;
use tokio::time::timeout;

const A2S_INFO_PAYLOAD: &[u8] = b"\xFF\xFF\xFF\xFFTSource Engine Query\0";
const HEADER_INFO_RESP: u8 = 0x49; // 'I'
const HEADER_CHALLENGE: u8 = 0x41; // 'A'

pub struct SteamProbe;

impl SteamProbe {
    pub fn new() -> Self { Self }
}
impl Default for SteamProbe {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl Probe for SteamProbe {
    async fn run(&self, monitor: &Monitor) -> Heartbeat {
        let started = Instant::now();
        let ts = OffsetDateTime::now_utc();
        let to = Duration::from_secs(monitor.timeout_seconds as u64);

        let Some(host) = monitor.hostname.as_deref() else {
            return down(monitor, ts, started, "steam monitor requires hostname");
        };
        let port = monitor.port.unwrap_or(27015) as u16;
        let addr = format!("{host}:{port}");

        let socket = match UdpSocket::bind("0.0.0.0:0").await {
            Ok(s) => s,
            Err(e) => return down(monitor, ts, started, &format!("udp bind: {e}")),
        };
        if let Err(e) = socket.connect(&addr).await {
            return down(monitor, ts, started, &format!("udp connect: {e}"));
        }

        // First attempt: bare A2S_INFO.
        if let Err(e) = socket.send(A2S_INFO_PAYLOAD).await {
            return down(monitor, ts, started, &format!("send: {e}"));
        }

        let mut buf = [0u8; 1400];
        let n = match timeout(to, socket.recv(&mut buf)).await {
            Ok(Ok(n)) => n,
            Ok(Err(e)) => return down(monitor, ts, started, &format!("recv: {e}")),
            Err(_) => return down(monitor, ts, started, "recv timed out"),
        };

        // Reply must start with 0xFFFFFFFF, then a header byte.
        if n < 5 || buf[..4] != [0xFF, 0xFF, 0xFF, 0xFF] {
            return down(monitor, ts, started, "malformed reply");
        }

        match buf[4] {
            HEADER_INFO_RESP => up(monitor, ts, started, "A2S_INFO reply"),
            HEADER_CHALLENGE if n >= 9 => {
                // Re-send with the 4-byte challenge appended.
                let mut payload = Vec::with_capacity(A2S_INFO_PAYLOAD.len() + 4);
                payload.extend_from_slice(A2S_INFO_PAYLOAD);
                payload.extend_from_slice(&buf[5..9]);
                if let Err(e) = socket.send(&payload).await {
                    return down(monitor, ts, started, &format!("send (challenge): {e}"));
                }
                let n2 = match timeout(to, socket.recv(&mut buf)).await {
                    Ok(Ok(n)) => n,
                    Ok(Err(e)) => return down(monitor, ts, started, &format!("recv (challenge): {e}")),
                    Err(_) => return down(monitor, ts, started, "challenge response timed out"),
                };
                if n2 >= 5 && buf[..4] == [0xFF, 0xFF, 0xFF, 0xFF] && buf[4] == HEADER_INFO_RESP {
                    up(monitor, ts, started, "A2S_INFO reply (post-challenge)")
                } else {
                    down(monitor, ts, started, "no info after challenge")
                }
            }
            other => down(monitor, ts, started, &format!("unexpected header 0x{other:02X}")),
        }
    }
}

fn up(monitor: &Monitor, ts: OffsetDateTime, started: Instant, msg: &str) -> Heartbeat {
    Heartbeat {
        monitor_id: monitor.id,
        ts,
        status: MonitorStatus::Up,
        latency_ms: Some(ms_i32(started.elapsed())),
        status_code: None,
        msg: Some(msg.into()),
        retries: 0,
        important: false,
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
