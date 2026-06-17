//! Spike.sh — POST integration URL with status + alert.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use rampart_core::MonitorStatus;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct SpikeShConfig {
    pub integration_url: String,
}

pub struct SpikeSh {
    cfg: SpikeShConfig,
    client: reqwest::Client,
}

impl SpikeSh {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: SpikeShConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if !cfg.integration_url.starts_with("http") {
            return Err(ChannelError::BadConfig("integration_url required".into()));
        }
        Ok(Self {
            cfg,
            client: crate::http::client(),
        })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    check_id: String,
    title: &'a str,
    message: &'a str,
    status: &'a str,
}

#[async_trait]
impl Channel for SpikeSh {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError> {
        let status = if event.heartbeat.status == MonitorStatus::Up {
            "resolved"
        } else {
            "triggered"
        };
        let payload = Payload {
            check_id: format!("rampart-monitor-{}", event.monitor.id.0),
            title: subject,
            message: body,
            status,
        };
        let resp = self
            .client
            .post(&self.cfg.integration_url)
            .json(&payload)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(
                resp.status().as_u16(),
                resp.text().await.unwrap_or_default(),
            ));
        }
        Ok(())
    }
}
