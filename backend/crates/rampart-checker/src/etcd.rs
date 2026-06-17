//! etcd health probe.
//!
//! GETs `{url}/health` — etcd's built-in endpoint returns `{"health":"true"}`
//! (string-typed boolean) when the member is healthy, and `{"health":"false",
//! "reason":"..."}` otherwise. Up only when `health == "true"`.
//!
//! `monitor.url` is the etcd client base (e.g. `http://etcd.internal:2379`);
//! optional `config.username` / `config.password` enable basic auth. The
//! client is SSRF-guarded (vetted at connect).

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use reqwest::Client;
use serde::Deserialize;
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tokio::time::timeout;

#[derive(Deserialize)]
struct EtcdHealth {
    // etcd serializes this as a STRING ("true"/"false"), not a JSON bool.
    #[serde(default)]
    health: String,
    #[serde(default)]
    reason: String,
}

pub struct EtcdProbe {
    client: once_cell::sync::OnceCell<Client>,
}

impl EtcdProbe {
    pub fn new() -> Self {
        Self {
            client: once_cell::sync::OnceCell::new(),
        }
    }

    fn client(&self) -> &Client {
        self.client.get_or_init(|| {
            crate::ssrf::guarded_client_builder()
                .pool_idle_timeout(Duration::from_secs(60))
                .build()
                .expect("reqwest client should build with default features")
        })
    }
}

impl Default for EtcdProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Probe for EtcdProbe {
    async fn run(&self, monitor: &Monitor) -> Heartbeat {
        let started = Instant::now();
        let ts = OffsetDateTime::now_utc();
        let to = Duration::from_secs(monitor.timeout_seconds as u64);

        let base = match monitor.url.as_deref() {
            Some(u) if !u.is_empty() => u.trim_end_matches('/'),
            _ => {
                return down(
                    monitor,
                    ts,
                    started,
                    "etcd monitor requires url (e.g. http://etcd.host:2379)",
                )
            }
        };
        let url = format!("{base}/health");

        let mut req = self.client().get(&url).timeout(to);
        if let Some(user) = monitor.config.get("username").and_then(|v| v.as_str()) {
            let pass = monitor
                .config
                .get("password")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            req = req.basic_auth(user, Some(pass));
        }

        let resp = match timeout(to, req.send()).await {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => return down(monitor, ts, started, &format!("request: {e}")),
            Err(_) => return down(monitor, ts, started, "request timed out"),
        };
        let code = resp.status().as_u16() as i32;
        if !resp.status().is_success() {
            return down(monitor, ts, started, &format!("http {code}"));
        }
        let body: EtcdHealth = match resp.json().await {
            Ok(b) => b,
            Err(e) => return down(monitor, ts, started, &format!("body parse: {e}")),
        };

        if body.health == "true" {
            Heartbeat {
                monitor_id: monitor.id,
                ts,
                status: MonitorStatus::Up,
                latency_ms: Some(ms_i32(started.elapsed())),
                status_code: Some(code),
                msg: Some("healthy".into()),
                retries: 0,
                important: false,
            }
        } else {
            let reason = if body.reason.is_empty() {
                "unhealthy".to_string()
            } else {
                format!("unhealthy · {}", body.reason)
            };
            down(monitor, ts, started, &reason)
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
