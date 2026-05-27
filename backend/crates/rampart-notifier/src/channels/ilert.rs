//! iLert — POST https://api.ilert.com/api/v1/events/<integration_key>
//! with event_type ALERT / RESOLVE.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use rampart_core::MonitorStatus;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct IlertConfig {
    pub integration_key: String,
}

pub struct Ilert { cfg: IlertConfig, client: reqwest::Client }

impl Ilert {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: IlertConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.integration_key.is_empty() {
            return Err(ChannelError::BadConfig("integration_key required".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    #[serde(rename = "apiKey")]
    api_key:    &'a str,
    #[serde(rename = "eventType")]
    event_type: &'a str,
    summary:    &'a str,
    details:    &'a str,
    #[serde(rename = "incidentKey")]
    incident_key: String,
}

#[async_trait]
impl Channel for Ilert {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError> {
        let kind = if event.heartbeat.status == MonitorStatus::Up { "RESOLVE" } else { "ALERT" };
        let payload = Payload {
            api_key: &self.cfg.integration_key,
            event_type: kind,
            summary: subject,
            details: body,
            incident_key: format!("rampart-monitor-{}", event.monitor.id.0),
        };
        let resp = self.client.post("https://api.ilert.com/api/v1/events")
            .json(&payload).send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }
}
