//! Zenduty — POST /api/events with API key header. Auto-resolves on
//! recovery via entity_id.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use rampart_core::MonitorStatus;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct ZendutyConfig {
    pub integration_url: String,
}

pub struct Zenduty {
    cfg: ZendutyConfig,
    client: reqwest::Client,
}

impl Zenduty {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: ZendutyConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if !cfg.integration_url.starts_with("http") {
            return Err(ChannelError::BadConfig("integration_url required".into()));
        }
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    alert_type: &'a str,
    message: &'a str,
    summary: &'a str,
    entity_id: String,
}

#[async_trait]
impl Channel for Zenduty {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError> {
        let kind = if event.heartbeat.status == MonitorStatus::Up {
            "resolved"
        } else {
            "critical"
        };
        let payload = Payload {
            alert_type: kind,
            message: subject,
            summary: body,
            entity_id: format!("rampart-monitor-{}", event.monitor.id.0),
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
