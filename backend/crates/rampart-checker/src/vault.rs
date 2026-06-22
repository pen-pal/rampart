//! HashiCorp Vault health probe.
//!
//! GETs `{url}/v1/sys/health` and maps Vault's documented status codes
//! (<https://developer.hashicorp.com/vault/api-docs/system/health>):
//!   * `200` → initialized, unsealed, active           → Up
//!   * `429` → unsealed but standby                     → Warn
//!   * `472` → disaster-recovery secondary active node  → Warn
//!   * `473` → performance standby                      → Warn
//!   * `501` → not initialized                          → Down
//!   * `503` → sealed                                   → Down
//!
//! `monitor.url` is the Vault base (e.g. `https://vault.internal:8200`). The
//! client is SSRF-guarded (vetted at connect). Vault returns these codes by
//! design, so we read the status code rather than the body.

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use reqwest::Client;
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tokio::time::timeout;

pub struct VaultProbe {
    client: once_cell::sync::OnceCell<Client>,
}

impl VaultProbe {
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

impl Default for VaultProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Probe for VaultProbe {
    async fn run(&self, monitor: &Monitor) -> Heartbeat {
        let started = Instant::now();
        let ts = OffsetDateTime::now_utc();
        let to = Duration::from_secs(monitor.timeout_seconds as u64);

        let base = match monitor.url.as_deref() {
            Some(u) if !u.is_empty() => u.trim_end_matches('/'),
            _ => {
                return hb(
                    monitor,
                    ts,
                    started,
                    MonitorStatus::Down,
                    None,
                    "vault monitor requires url (e.g. https://vault.host:8200)",
                )
            }
        };
        // standbyok + perfstandbyok make standby nodes return 200 only if the
        // operator wants that; we instead read the raw code so the heartbeat
        // distinguishes active vs standby. uninitcode/sealedcode left default.
        let url = format!("{base}/v1/sys/health");

        let resp = match timeout(to, self.client().get(&url).timeout(to).send()).await {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => {
                return hb(
                    monitor,
                    ts,
                    started,
                    MonitorStatus::Down,
                    None,
                    &format!("request: {e}"),
                )
            }
            Err(_) => {
                return hb(
                    monitor,
                    ts,
                    started,
                    MonitorStatus::Down,
                    None,
                    "request timed out",
                )
            }
        };
        let code = resp.status().as_u16() as i32;
        let (status, msg) = match code {
            200 => (MonitorStatus::Up, "unsealed · active"),
            429 => (MonitorStatus::Warn, "unsealed · standby"),
            472 => (MonitorStatus::Warn, "DR secondary active"),
            473 => (MonitorStatus::Warn, "performance standby"),
            501 => (MonitorStatus::Down, "not initialized"),
            503 => (MonitorStatus::Down, "sealed"),
            _ => (MonitorStatus::Down, "unexpected status"),
        };
        hb(monitor, ts, started, status, Some(code), msg)
    }
}

fn hb(
    monitor: &Monitor,
    ts: OffsetDateTime,
    started: Instant,
    status: MonitorStatus,
    code: Option<i32>,
    msg: &str,
) -> Heartbeat {
    Heartbeat {
        monitor_id: monitor.id,
        ts,
        status,
        latency_ms: Some(ms_i32(started.elapsed())),
        status_code: code,
        msg: Some(msg.into()),
        retries: 0,
        important: false,
    }
}
