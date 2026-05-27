//! PagerTree — integration URL accepts a JSON alert payload.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use rampart_core::MonitorStatus;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct PagertreeConfig {
    pub integration_url: String,
    #[serde(default = "default_severity")]
    pub severity: String,
}
fn default_severity() -> String { "SEV-3".into() }

pub struct Pagertree { cfg: PagertreeConfig, client: reqwest::Client }

impl Pagertree {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: PagertreeConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if !cfg.integration_url.starts_with("http") {
            return Err(ChannelError::BadConfig("integration_url required".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    event_type:  &'a str,
    incident_id: String,
    title:       &'a str,
    description: &'a str,
    severity:    &'a str,
}

#[async_trait]
impl Channel for Pagertree {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError> {
        let kind = match event.heartbeat.status {
            MonitorStatus::Up => "resolve",
            _                 => "create",
        };
        let payload = Payload {
            event_type:  kind,
            incident_id: format!("rampart-monitor-{}", event.monitor.id.0),
            title:       subject,
            description: body,
            severity:    &self.cfg.severity,
        };
        let resp = self.client.post(&self.cfg.integration_url)
            .json(&payload)
            .send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }
}
