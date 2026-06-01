//! Memcached probe — text-protocol "version" handshake.
//!
//! TCP connects to host:port, writes `version\r\n`, reads a response,
//! and asserts that it starts with the expected prefix. Default prefix
//! is `VERSION ` (per the memcached protocol), but operators can
//! override via `config.expect` to support drop-in alternates (mcrouter,
//! Twemproxy, Couchbase memcached buckets) whose version strings differ.
//!
//! Doesn't authenticate: memcached's SASL is optional and rarely on,
//! and this probe is meant to confirm the server is accepting text-
//! protocol commands. Real auth would belong in a separate probe.

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

const DEFAULT_PORT: u16 = 11211;
const DEFAULT_EXPECT: &str = "VERSION ";

pub struct MemcachedProbe;

impl MemcachedProbe {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MemcachedProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Probe for MemcachedProbe {
    async fn run(&self, monitor: &Monitor) -> Heartbeat {
        let started = Instant::now();
        let ts = OffsetDateTime::now_utc();
        let to = Duration::from_secs(monitor.timeout_seconds as u64);

        let Some(host) = monitor.hostname.as_deref() else {
            return down(monitor, ts, started, "memcached monitor requires a hostname");
        };
        let port = monitor.port.map(|p| p as u16).unwrap_or(DEFAULT_PORT);
        let expect = monitor
            .config
            .get("expect")
            .and_then(|v| v.as_str())
            .unwrap_or(DEFAULT_EXPECT);

        let addr = format!("{host}:{port}");
        let stream = match timeout(to, TcpStream::connect(&addr)).await {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => return down(monitor, ts, started, &format!("connect failed: {e}")),
            Err(_) => return down(monitor, ts, started, "connect timed out"),
        };
        let mut stream = stream;
        if let Err(e) = timeout(to, stream.write_all(b"version\r\n")).await {
            return down(monitor, ts, started, &format!("write timed out: {e}"));
        }

        // Read a small chunk — the version response is well under 64 bytes.
        let mut buf = [0u8; 128];
        let n = match timeout(to, stream.read(&mut buf)).await {
            Ok(Ok(0))  => return down(monitor, ts, started, "server closed connection"),
            Ok(Ok(n))  => n,
            Ok(Err(e)) => return down(monitor, ts, started, &format!("read failed: {e}")),
            Err(_)     => return down(monitor, ts, started, "read timed out"),
        };
        let resp = String::from_utf8_lossy(&buf[..n]);
        let head = resp.lines().next().unwrap_or("").trim_end_matches('\r');

        if !head.starts_with(expect) {
            return down(
                monitor,
                ts,
                started,
                &format!("unexpected greeting (got {head:?}, want prefix {expect:?})"),
            );
        }

        Heartbeat {
            monitor_id: monitor.id,
            ts,
            status: MonitorStatus::Up,
            latency_ms: Some(ms_i32(started.elapsed())),
            status_code: None,
            msg: Some(head.to_string()),
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
