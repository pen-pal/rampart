//! Elasticsearch / OpenSearch cluster-health probe.
//!
//! GETs `{url}/_cluster/health` and reads the cluster `status`:
//!   * `green`  → Up
//!   * `yellow` → Warn (or Up when `config.allow_yellow` is set — a single-node
//!     dev cluster is permanently yellow because replicas can't be allocated)
//!   * `red`    → Down
//!
//! `monitor.url` is the cluster base (e.g. `https://es.internal:9200`);
//! optional `config.username` / `config.password` enable HTTP basic auth.
//! The client is SSRF-guarded (vetted at connect), like the other probes.

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use reqwest::Client;
use serde::Deserialize;
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tokio::time::timeout;

#[derive(Deserialize)]
struct EsHealth {
    status: String,
    #[serde(default)]
    cluster_name: String,
}

pub struct ElasticsearchProbe {
    client: once_cell::sync::OnceCell<Client>,
}

impl ElasticsearchProbe {
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

impl Default for ElasticsearchProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Probe for ElasticsearchProbe {
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
                    "elasticsearch monitor requires url (e.g. https://es.host:9200)",
                )
            }
        };
        let url = format!("{base}/_cluster/health");

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
        let body: EsHealth = match resp.json().await {
            Ok(b) => b,
            Err(e) => return down(monitor, ts, started, &format!("body parse: {e}")),
        };

        let allow_yellow = monitor
            .config
            .get("allow_yellow")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let name = if body.cluster_name.is_empty() {
            String::new()
        } else {
            format!(" · {}", body.cluster_name)
        };
        let (status, msg) = match body.status.as_str() {
            "green" => (MonitorStatus::Up, format!("green{name}")),
            "yellow" if allow_yellow => (MonitorStatus::Up, format!("yellow{name}")),
            "yellow" => (MonitorStatus::Warn, format!("yellow{name}")),
            "red" => (MonitorStatus::Down, format!("red{name}")),
            other => (MonitorStatus::Warn, format!("unknown status '{other}'")),
        };
        Heartbeat {
            monitor_id: monitor.id,
            ts,
            status,
            latency_ms: Some(ms_i32(started.elapsed())),
            status_code: Some(code),
            msg: Some(msg),
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
