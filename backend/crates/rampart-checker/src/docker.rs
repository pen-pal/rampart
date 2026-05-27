//! Docker container probe.
//!
//! Connects to the Docker daemon (default: local Unix socket) and
//! inspects the named container. Up when `State.Running == true`,
//! Warn when the container exists but isn't running, Down when the
//! daemon is unreachable or the container is missing.
//!
//! Config:
//!
//! ```json
//! {
//!   "container": "plex",          // name or id; required
//!   "host":      "/var/run/docker.sock"  // optional override
//! }
//! ```

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use bollard::container::InspectContainerOptions;
use bollard::Docker;
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tokio::time::timeout;

pub struct DockerProbe;

impl DockerProbe {
    pub fn new() -> Self { Self }
}
impl Default for DockerProbe {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl Probe for DockerProbe {
    async fn run(&self, monitor: &Monitor) -> Heartbeat {
        let started = Instant::now();
        let ts = OffsetDateTime::now_utc();
        let to = Duration::from_secs(monitor.timeout_seconds as u64);

        let container = monitor
            .config
            .get("container")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let Some(container) = container else {
            return down(monitor, ts, started, "docker monitor requires config.container");
        };

        // Defaults — env vars (DOCKER_HOST) work too via Docker::connect_with_defaults.
        let host_override = monitor.config.get("host").and_then(|v| v.as_str());

        let client = match host_override {
            Some(h) if h.starts_with("http") || h.starts_with("tcp") => {
                Docker::connect_with_http(h, 5, bollard::API_DEFAULT_VERSION)
            }
            Some(h) => Docker::connect_with_socket(h, 5, bollard::API_DEFAULT_VERSION),
            None => Docker::connect_with_defaults(),
        };
        let client = match client {
            Ok(c) => c,
            Err(e) => return down(monitor, ts, started, &format!("docker client: {e}")),
        };

        let inspect = client.inspect_container(&container, None::<InspectContainerOptions>);
        match timeout(to, inspect).await {
            Ok(Ok(info)) => {
                let state = info.state.unwrap_or_default();
                let running = state.running.unwrap_or(false);
                let raw = state.status.map(|s| format!("{s:?}")).unwrap_or_else(|| "?".into());
                if running {
                    Heartbeat {
                        monitor_id: monitor.id,
                        ts,
                        status: MonitorStatus::Up,
                        latency_ms: Some(ms_i32(started.elapsed())),
                        status_code: None,
                        msg: Some(format!("{container}: {raw}")),
                        retries: 0,
                        important: false,
                    }
                } else {
                    Heartbeat {
                        monitor_id: monitor.id,
                        ts,
                        status: MonitorStatus::Down,
                        latency_ms: Some(ms_i32(started.elapsed())),
                        status_code: None,
                        msg: Some(format!("{container}: not running ({raw})")),
                        retries: 0,
                        important: false,
                    }
                }
            }
            Ok(Err(e)) => down(monitor, ts, started, &format!("inspect: {e}")),
            Err(_) => down(monitor, ts, started, "inspect timed out"),
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
