//! gRPC probe — `grpc.health.v1.Health/Check`.
//!
//! The target service must implement the standard health protocol
//! (https://github.com/grpc/grpc/blob/master/doc/health-checking.md);
//! tonic-health gives us a pre-built client. We don't authenticate or
//! send TLS by default — for `grpcs://` endpoints, set `monitor.url`
//! to a full https://host:port URI.
//!
//! Config:
//!
//! ```json
//! { "service": "" }   // service name to ask about; default "" (server overall)
//! ```

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tokio::time::timeout;
use tonic_health::pb::health_check_response::ServingStatus;
use tonic_health::pb::health_client::HealthClient;
use tonic_health::pb::HealthCheckRequest;

pub struct GrpcProbe;

impl GrpcProbe {
    pub fn new() -> Self {
        Self
    }
}
impl Default for GrpcProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Probe for GrpcProbe {
    async fn run(&self, monitor: &Monitor) -> Heartbeat {
        let started = Instant::now();
        let ts = OffsetDateTime::now_utc();
        let to = Duration::from_secs(monitor.timeout_seconds as u64);

        // Allow either a full URL (preferred for TLS) or hostname+port.
        let endpoint = match monitor.url.as_deref() {
            Some(u) if !u.is_empty() => u.to_string(),
            _ => match (monitor.hostname.as_deref(), monitor.port) {
                (Some(h), Some(p)) => format!("http://{h}:{p}"),
                _ => {
                    return down(
                        monitor,
                        ts,
                        started,
                        "grpc monitor requires url or hostname+port",
                    )
                }
            },
        };

        let service = monitor
            .config
            .get("service")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let chan_fut = tonic::transport::Endpoint::from_shared(endpoint)
            .map(|e| e.timeout(to).connect_timeout(to));
        let endpoint = match chan_fut {
            Ok(e) => e,
            Err(e) => return down(monitor, ts, started, &format!("bad endpoint: {e}")),
        };

        let channel = match timeout(to, endpoint.connect()).await {
            Ok(Ok(c)) => c,
            Ok(Err(e)) => return down(monitor, ts, started, &format!("connect: {e}")),
            Err(_) => return down(monitor, ts, started, "connect timed out"),
        };

        let mut client = HealthClient::new(channel);
        let req = tonic::Request::new(HealthCheckRequest { service });
        match timeout(to, client.check(req)).await {
            Ok(Ok(resp)) => {
                let status = resp.into_inner().status();
                map_status(monitor, ts, started, status)
            }
            Ok(Err(e)) => down(monitor, ts, started, &format!("check: {e}")),
            Err(_) => down(monitor, ts, started, "check timed out"),
        }
    }
}

fn map_status(
    monitor: &Monitor,
    ts: OffsetDateTime,
    started: Instant,
    status: ServingStatus,
) -> Heartbeat {
    let (s, msg) = match status {
        ServingStatus::Serving => (MonitorStatus::Up, "SERVING"),
        ServingStatus::NotServing => (MonitorStatus::Down, "NOT_SERVING"),
        ServingStatus::ServiceUnknown => (MonitorStatus::Warn, "SERVICE_UNKNOWN"),
        ServingStatus::Unknown => (MonitorStatus::Warn, "UNKNOWN"),
    };
    Heartbeat {
        monitor_id: monitor.id,
        ts,
        status: s,
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
