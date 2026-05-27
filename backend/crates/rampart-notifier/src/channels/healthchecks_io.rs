//! Healthchecks.io — ping URL. Up sends /success, Down sends /fail.
//! Allows running Rampart in front of Healthchecks for chained alerting.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use rampart_core::MonitorStatus;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct HcConfig {
    /// e.g. https://hc-ping.com/<uuid>
    pub ping_url: String,
}

pub struct HealthchecksIo { cfg: HcConfig, client: reqwest::Client }

impl HealthchecksIo {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: HcConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if !cfg.ping_url.starts_with("http") {
            return Err(ChannelError::BadConfig("ping_url required".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[async_trait]
impl Channel for HealthchecksIo {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError> {
        let base = self.cfg.ping_url.trim_end_matches('/');
        let url = if event.heartbeat.status == MonitorStatus::Up {
            base.to_string()  // success ping
        } else {
            format!("{base}/fail")
        };
        let resp = self.client.post(&url)
            .body(format!("{subject}\n{body}"))
            .send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }
}
