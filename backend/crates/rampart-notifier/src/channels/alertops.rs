//! AlertOps — POST integration_url with subject + status_message + source.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use rampart_core::MonitorStatus;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct AlertopsConfig {
    pub integration_url: String,
}

pub struct Alertops {
    cfg: AlertopsConfig,
    client: reqwest::Client,
}

impl Alertops {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: AlertopsConfig = serde_json::from_value(raw.clone())
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
    subject: &'a str,
    status_message: &'a str,
    source: &'static str,
    source_id: String,
    severity: &'static str,
    action: &'static str,
}

#[async_trait]
impl Channel for Alertops {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError> {
        let action = if event.heartbeat.status == MonitorStatus::Up {
            "Close"
        } else {
            "Open"
        };
        let payload = Payload {
            subject,
            status_message: body,
            source: "rampart",
            source_id: format!("rampart-monitor-{}", event.monitor.id.0),
            severity: "High",
            action,
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
