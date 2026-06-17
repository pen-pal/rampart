//! Splunk On-Call (formerly VictorOps) — REST integration URL.
//!
//! Routing key is part of the URL. Splits trigger / resolve via
//! the `message_type` field.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use rampart_core::MonitorStatus;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct SplunkConfig {
    pub integration_url: String,
}

pub struct Splunk {
    cfg: SplunkConfig,
    client: reqwest::Client,
}

impl Splunk {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: SplunkConfig = serde_json::from_value(raw.clone())
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
    message_type: &'a str,
    entity_id: String,
    state_message: String,
    monitoring_tool: &'static str,
    entity_display_name: &'a str,
}

#[async_trait]
impl Channel for Splunk {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError> {
        let kind = match event.heartbeat.status {
            MonitorStatus::Up => "RECOVERY",
            MonitorStatus::Warn => "WARNING",
            _ => "CRITICAL",
        };
        let payload = Payload {
            message_type: kind,
            entity_id: format!("rampart-monitor-{}", event.monitor.id.0),
            state_message: format!("{subject}\n{body}"),
            monitoring_tool: "rampart",
            entity_display_name: &event.monitor.name,
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
