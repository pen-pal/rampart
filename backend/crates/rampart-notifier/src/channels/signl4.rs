//! SIGNL4 — POST team integration URL with title / message / severity.
//! https://signl4.com — webhook URL contains the team secret.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use rampart_core::MonitorStatus;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct Signl4Config {
    pub team_secret: String,
}

pub struct Signl4 { cfg: Signl4Config, client: reqwest::Client }

impl Signl4 {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: Signl4Config = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.team_secret.trim().is_empty() {
            return Err(ChannelError::BadConfig("team_secret required".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    #[serde(rename = "Title")]
    title:    &'a str,
    #[serde(rename = "Message")]
    message:  &'a str,
    #[serde(rename = "X-S4-ExternalID")]
    ext_id:   String,
    #[serde(rename = "X-S4-Status")]
    status:   &'static str,
}

#[async_trait]
impl Channel for Signl4 {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError> {
        let status = match event.heartbeat.status {
            MonitorStatus::Up => "resolved",
            _                  => "new",
        };
        let payload = Payload {
            title: subject,
            message: body,
            ext_id: format!("rampart-monitor-{}", event.monitor.id.0),
            status,
        };
        let url = format!("https://connect.signl4.com/webhook/{}", self.cfg.team_secret);
        let resp = self.client.post(&url).json(&payload).send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }
}
