//! Squadcast — webhook integration URL (incidents.app/v2/incidents/...).
//!
//! Squadcast accepts a generic webhook payload + event_id for dedupe.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use rampart_core::MonitorStatus;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct SquadcastConfig {
    pub webhook_url: String,
}

pub struct Squadcast {
    cfg: SquadcastConfig,
    client: reqwest::Client,
}

impl Squadcast {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: SquadcastConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if !cfg.webhook_url.starts_with("http") {
            return Err(ChannelError::BadConfig("webhook_url required".into()));
        }
        Ok(Self {
            cfg,
            client: crate::http::client(),
        })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    message: &'a str,
    description: &'a str,
    status: &'a str,
    event_id: String,
}

#[async_trait]
impl Channel for Squadcast {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError> {
        let status = if event.heartbeat.status == MonitorStatus::Up {
            "resolve"
        } else {
            "trigger"
        };
        let payload = Payload {
            message: subject,
            description: body,
            status,
            event_id: format!("rampart-monitor-{}", event.monitor.id.0),
        };
        let resp = self
            .client
            .post(&self.cfg.webhook_url)
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
