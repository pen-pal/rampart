//! Kafka broker probe — raw `ApiVersions` request.
//!
//! Skips librdkafka entirely. ApiVersions (key=18, v0) is the cheapest
//! valid Kafka request: a connected client sends it before anything else
//! to negotiate protocol versions, and every broker handles it without
//! authentication. We parse the 2-byte error code from the response —
//! 0 means a healthy broker.
//!
//! Wire format reference: https://kafka.apache.org/protocol#api_versions

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

pub struct KafkaProbe;

impl KafkaProbe {
    pub fn new() -> Self {
        Self
    }
}
impl Default for KafkaProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Probe for KafkaProbe {
    async fn run(&self, monitor: &Monitor) -> Heartbeat {
        let started = Instant::now();
        let ts = OffsetDateTime::now_utc();
        let to = Duration::from_secs(monitor.timeout_seconds as u64);

        let Some(host) = monitor.hostname.as_deref() else {
            return down(monitor, ts, started, "kafka monitor requires hostname");
        };
        let port = monitor.port.unwrap_or(9092) as u16;

        let stream = match timeout(to, TcpStream::connect((host, port))).await {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => return down(monitor, ts, started, &format!("tcp: {e}")),
            Err(_) => return down(monitor, ts, started, "tcp connect timed out"),
        };

        match timeout(to, exchange(stream)).await {
            Ok(Ok(())) => Heartbeat {
                monitor_id: monitor.id,
                ts,
                status: MonitorStatus::Up,
                latency_ms: Some(ms_i32(started.elapsed())),
                status_code: None,
                msg: Some("ApiVersions ok".into()),
                retries: 0,
                important: false,
            },
            Ok(Err(e)) => down(monitor, ts, started, &e),
            Err(_) => down(monitor, ts, started, "broker handshake timed out"),
        }
    }
}

/// One ApiVersions request/response round-trip.
async fn exchange(mut stream: TcpStream) -> Result<(), String> {
    // Build the request payload (everything after the 4-byte size header).
    //   api_key       i16  = 18
    //   api_version   i16  = 0
    //   correlation   i32  = 1
    //   client_id     str  = "rampart" (i16 length prefix)
    let client_id = b"rampart";
    let mut body = Vec::with_capacity(16);
    body.extend_from_slice(&18i16.to_be_bytes());
    body.extend_from_slice(&0i16.to_be_bytes());
    body.extend_from_slice(&1i32.to_be_bytes());
    body.extend_from_slice(&(client_id.len() as i16).to_be_bytes());
    body.extend_from_slice(client_id);

    let mut frame = Vec::with_capacity(4 + body.len());
    frame.extend_from_slice(&(body.len() as i32).to_be_bytes());
    frame.extend_from_slice(&body);

    stream
        .write_all(&frame)
        .await
        .map_err(|e| format!("write: {e}"))?;
    stream.flush().await.map_err(|e| format!("flush: {e}"))?;

    // Response: 4-byte size, 4-byte correlation, 2-byte error code, ...
    let mut size_buf = [0u8; 4];
    stream
        .read_exact(&mut size_buf)
        .await
        .map_err(|e| format!("read size: {e}"))?;
    let size = i32::from_be_bytes(size_buf);
    if !(6..=65536).contains(&size) {
        return Err(format!("implausible response size {size}"));
    }

    // Read header (correlation + error code).
    let mut hdr = [0u8; 6];
    stream
        .read_exact(&mut hdr)
        .await
        .map_err(|e| format!("read hdr: {e}"))?;
    let err = i16::from_be_bytes([hdr[4], hdr[5]]);
    if err != 0 {
        return Err(format!("broker error code {err}"));
    }

    // Drain the rest so we don't half-close the connection on the
    // broker's side mid-write — they log this otherwise.
    let remaining = (size as usize).saturating_sub(6);
    if remaining > 0 {
        let sink_len = remaining.min(8192);
        let mut sink = vec![0u8; sink_len];
        let mut left = remaining;
        while left > 0 {
            let take = left.min(sink_len);
            let n = stream
                .read(&mut sink[..take])
                .await
                .map_err(|e| format!("drain: {e}"))?;
            if n == 0 {
                break;
            }
            left = left.saturating_sub(n);
        }
    }
    Ok(())
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
