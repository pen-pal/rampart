//! AlertNow — POST integration URL with a JSON payload.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use rampart_core::MonitorStatus;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct AlertNowConfig {
    pub webhook_url: String,
}

pub struct AlertNow { cfg: AlertNowConfig, client: reqwest::Client }

impl AlertNow {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: AlertNowConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if !cfg.webhook_url.starts_with("http") {
            return Err(ChannelError::BadConfig("webhook_url required".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    event_id: String,
    summary:  &'a str,
    detail:   &'a str,
    severity: &'a str,
    state:    &'a str,
}

#[async_trait]
impl Channel for AlertNow {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError> {
        let (severity, state) = match event.heartbeat.status {
            MonitorStatus::Up   => ("low",  "resolved"),
            MonitorStatus::Warn => ("medium","triggered"),
            _                    => ("high", "triggered"),
        };
        let payload = Payload {
            event_id: format!("rampart-monitor-{}", event.monitor.id.0),
            summary: subject,
            detail: body,
            severity,
            state,
        };
        let resp = self.client.post(&self.cfg.webhook_url).json(&payload).send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }
}
