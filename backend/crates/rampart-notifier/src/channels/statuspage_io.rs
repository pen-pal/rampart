//! Atlassian Statuspage.io — create incidents on a page.
//! POST /v1/pages/<page_id>/incidents with OAuth bearer or API key.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use rampart_core::MonitorStatus;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct StatuspageConfig {
    pub api_key: String,
    pub page_id: String,
}

pub struct StatuspageIo { cfg: StatuspageConfig, client: reqwest::Client }

impl StatuspageIo {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: StatuspageConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.api_key.is_empty() || cfg.page_id.is_empty() {
            return Err(ChannelError::BadConfig("api_key + page_id required".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[derive(Serialize)]
struct Payload<'a> { incident: Inc<'a> }
#[derive(Serialize)]
struct Inc<'a> {
    name:    &'a str,
    status:  &'a str,
    body:    &'a str,
    impact_override: &'static str,
}

#[async_trait]
impl Channel for StatuspageIo {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError> {
        let status = if event.heartbeat.status == MonitorStatus::Up { "resolved" } else { "investigating" };
        let url = format!("https://api.statuspage.io/v1/pages/{}/incidents", self.cfg.page_id);
        let payload = Payload {
            incident: Inc {
                name: subject,
                status,
                body,
                impact_override: "minor",
            },
        };
        let resp = self.client.post(&url)
            .header("Authorization", format!("OAuth {}", self.cfg.api_key))
            .json(&payload).send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }
}
