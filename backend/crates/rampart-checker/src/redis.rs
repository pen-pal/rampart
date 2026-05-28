//! Redis probe — connect, send `PING`, expect `PONG`.
//!
//! More useful than a raw TCP probe because the AUTH handshake (when
//! a password is configured) lives inside the Redis protocol — TCP
//! alone would pass even if the password were wrong.
//!
//! Connection URL precedence: `monitor.config.connection_string`,
//! otherwise built from hostname/port/password/db. Use the
//! `rediss://` scheme to enable TLS.

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tokio::time::timeout;

pub struct RedisProbe;

impl RedisProbe {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RedisProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Probe for RedisProbe {
    async fn run(&self, monitor: &Monitor) -> Heartbeat {
        let started = Instant::now();
        let ts = OffsetDateTime::now_utc();
        let to = Duration::from_secs(monitor.timeout_seconds as u64);

        let url = match build_url(monitor) {
            Ok(u) => u,
            Err(e) => return down(monitor, ts, started, &e),
        };

        let client = match redis::Client::open(url) {
            Ok(c) => c,
            Err(e) => return down(monitor, ts, started, &format!("client: {e}")),
        };

        let conn_fut = client.get_multiplexed_async_connection();
        let mut conn = match timeout(to, conn_fut).await {
            Ok(Ok(c)) => c,
            Ok(Err(e)) => return down(monitor, ts, started, &format!("connect: {e}")),
            Err(_) => return down(monitor, ts, started, "connect timed out"),
        };

        // PING returns the bulk-string "PONG" — accept any successful
        // round-trip rather than asserting the literal value, since some
        // proxies (Envoy, Twemproxy) return slightly different responses.
        let cmd = redis::cmd("PING");
        let ping_fut = cmd.query_async::<String>(&mut conn);
        match timeout(to, ping_fut).await {
            Ok(Ok(s)) => Heartbeat {
                monitor_id: monitor.id,
                ts,
                status: MonitorStatus::Up,
                latency_ms: Some(ms_i32(started.elapsed())),
                status_code: None,
                msg: Some(format!("PING → {s}")),
                retries: 0,
                important: false,
            },
            Ok(Err(e)) => down(monitor, ts, started, &format!("PING: {e}")),
            Err(_) => down(monitor, ts, started, "PING timed out"),
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

fn build_url(monitor: &Monitor) -> Result<String, String> {
    if let Some(s) = monitor
        .config
        .get("connection_string")
        .and_then(|v| v.as_str())
    {
        if !s.is_empty() {
            return Ok(s.to_string());
        }
    }
    let host = monitor
        .hostname
        .as_deref()
        .ok_or_else(|| "redis monitor requires hostname or connection_string".to_string())?;
    let port = monitor.port.unwrap_or(6379) as u16;

    let scheme = match monitor.config.get("tls").and_then(|v| v.as_bool()) {
        Some(true) => "rediss",
        _ => "redis",
    };
    let creds = match monitor.config.get("password").and_then(|v| v.as_str()) {
        Some(p) if !p.is_empty() => format!(":{p}@"),
        _ => String::new(),
    };
    let db = monitor
        .config
        .get("db")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    Ok(format!("{scheme}://{creds}{host}:{port}/{db}"))
}
