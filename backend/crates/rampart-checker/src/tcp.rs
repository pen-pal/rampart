//! TCP probe — connect, measure handshake latency.

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tokio::net::TcpStream;
use tokio::time::timeout;

pub struct TcpProbe;

impl TcpProbe {
    pub fn new() -> Self { Self }
}

impl Default for TcpProbe {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl Probe for TcpProbe {
    async fn run(&self, monitor: &Monitor) -> Heartbeat {
        let started = Instant::now();
        let ts = OffsetDateTime::now_utc();
        let to = Duration::from_secs(monitor.timeout_seconds as u64);

        // TCP needs hostname + port. If either is missing, fail loudly.
        let (host, port) = match (&monitor.hostname, monitor.port) {
            (Some(h), Some(p)) => (h.clone(), p),
            _ => return Heartbeat {
                monitor_id: monitor.id, ts,
                status: MonitorStatus::Down,
                latency_ms: None, status_code: None,
                msg: Some("tcp monitor requires hostname + port".into()),
                retries: 0, important: false,
            },
        };
        let addr = format!("{host}:{port}");

        match timeout(to, TcpStream::connect(&addr)).await {
            Ok(Ok(_stream)) => Heartbeat {
                monitor_id: monitor.id, ts,
                status:     MonitorStatus::Up,
                latency_ms: Some(ms_i32(started.elapsed())),
                status_code: None, msg: None,
                retries: 0, important: false,
            },
            Ok(Err(e)) => Heartbeat {
                monitor_id: monitor.id, ts,
                status:     MonitorStatus::Down,
                latency_ms: Some(ms_i32(started.elapsed())),
                status_code: None,
                msg: Some(e.to_string()),
                retries: 0, important: false,
            },
            Err(_) => Heartbeat {
                monitor_id: monitor.id, ts,
                status:     MonitorStatus::Down,
                latency_ms: Some(ms_i32(started.elapsed())),
                status_code: None,
                msg: Some("tcp connect timed out".into()),
                retries: 0, important: false,
            },
        }
    }
}
