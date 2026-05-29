//! Banner-protocol probe — SSH, SMTP, IMAP.
//!
//! All three greet the client on connect, so the check is: open TCP,
//! read the first line within the timeout, and confirm it starts with
//! the protocol's expected prefix. `config.expect` overrides the prefix
//! (e.g. pin a specific SMTP banner or SSH server-version string).
//!
//! Defaults:
//!   ssh  → "SSH-"
//!   smtp → "220"
//!   imap → "* OK"

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use rampart_core::{Heartbeat, Monitor, MonitorKind, MonitorStatus};
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tokio::io::{AsyncReadExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::timeout;

pub struct BannerProbe;

impl BannerProbe {
    pub fn new() -> Self {
        Self
    }
}

impl Default for BannerProbe {
    fn default() -> Self {
        Self::new()
    }
}

fn default_expect(kind: MonitorKind) -> &'static str {
    match kind {
        MonitorKind::Ssh => "SSH-",
        MonitorKind::Smtp => "220",
        MonitorKind::Imap => "* OK",
        _ => "",
    }
}

fn default_port(kind: MonitorKind) -> u16 {
    match kind {
        MonitorKind::Ssh => 22,
        MonitorKind::Smtp => 25,
        MonitorKind::Imap => 143,
        _ => 0,
    }
}

#[async_trait]
impl Probe for BannerProbe {
    async fn run(&self, monitor: &Monitor) -> Heartbeat {
        let started = Instant::now();
        let ts = OffsetDateTime::now_utc();
        let to = Duration::from_secs(monitor.timeout_seconds as u64);

        let down = |latency: Option<i32>, msg: String| Heartbeat {
            monitor_id: monitor.id,
            ts,
            status: MonitorStatus::Down,
            latency_ms: latency,
            status_code: None,
            msg: Some(msg),
            retries: 0,
            important: false,
        };

        let host = match &monitor.hostname {
            Some(h) if !h.is_empty() => h.clone(),
            _ => return down(None, "banner monitor requires a hostname".into()),
        };
        let port = monitor.port.unwrap_or(default_port(monitor.kind) as i32);
        let addr = format!("{host}:{port}");

        let expect = monitor
            .config
            .get("expect")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| default_expect(monitor.kind).to_string());

        // Connect.
        let stream = match timeout(to, TcpStream::connect(&addr)).await {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => return down(Some(ms_i32(started.elapsed())), format!("connect: {e}")),
            Err(_) => return down(Some(ms_i32(started.elapsed())), "connect timed out".into()),
        };

        // Read the greeting (first chunk). Banner protocols send it
        // immediately; we don't need a full line parser — the prefix is
        // in the first bytes.
        let mut reader = BufReader::new(stream);
        let mut buf = [0u8; 256];
        let n = match timeout(to, reader.read(&mut buf)).await {
            Ok(Ok(0)) => {
                return down(
                    Some(ms_i32(started.elapsed())),
                    "connection closed before greeting".into(),
                )
            }
            Ok(Ok(n)) => n,
            Ok(Err(e)) => return down(Some(ms_i32(started.elapsed())), format!("read: {e}")),
            Err(_) => {
                return down(
                    Some(ms_i32(started.elapsed())),
                    "no greeting before timeout".into(),
                )
            }
        };
        let greeting = String::from_utf8_lossy(&buf[..n]);
        let first_line = greeting.lines().next().unwrap_or("").trim();
        let latency = ms_i32(started.elapsed());

        if first_line.starts_with(&expect) {
            Heartbeat {
                monitor_id: monitor.id,
                ts,
                status: MonitorStatus::Up,
                latency_ms: Some(latency),
                status_code: None,
                msg: Some(first_line.chars().take(120).collect()),
                retries: 0,
                important: false,
            }
        } else {
            down(
                Some(latency),
                format!(
                    "unexpected greeting (want prefix '{expect}'): {}",
                    first_line.chars().take(80).collect::<String>()
                ),
            )
        }
    }
}
