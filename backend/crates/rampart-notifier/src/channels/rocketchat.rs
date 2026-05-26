//! Rocket.Chat incoming webhook.
//!
//! Also Slack-compatible — same shape as Mattermost. Setup: in Rocket.Chat
//! Admin → Integrations → New → Incoming. Configure and grab the URL.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct RocketChatConfig {
    pub webhook_url: String,
    #[serde(default)]
    pub channel:     Option<String>,
    #[serde(default)]
    pub alias:       Option<String>,
    #[serde(default)]
    pub avatar:      Option<String>,
    #[serde(default)]
    pub emoji:       Option<String>,
}

pub struct RocketChat {
    cfg:    RocketChatConfig,
    client: reqwest::Client,
}

impl RocketChat {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: RocketChatConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if !cfg.webhook_url.starts_with("https://") && !cfg.webhook_url.starts_with("http://") {
            return Err(ChannelError::BadConfig("webhook_url must start with http(s)://".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[derive(Serialize)]
struct RcPayload<'a> {
    text:        String,
    #[serde(skip_serializing_if = "Option::is_none")]
    channel:     &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    alias:       &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    avatar:      &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    emoji:       &'a Option<String>,
    attachments: Vec<RcAttachment>,
}

#[derive(Serialize)]
struct RcAttachment {
    color: String,
    title: String,
    text:  String,
}

#[async_trait]
impl Channel for RocketChat {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError> {
        let color = match event.heartbeat.status {
            rampart_core::MonitorStatus::Up   => "#10b981",
            rampart_core::MonitorStatus::Down => "#ef4444",
            _                                 => "#f59e0b",
        }.to_string();
        let payload = RcPayload {
            text:        subject.to_string(),
            channel:     &self.cfg.channel,
            alias:       &self.cfg.alias,
            avatar:      &self.cfg.avatar,
            emoji:       &self.cfg.emoji,
            attachments: vec![RcAttachment {
                color, title: subject.to_string(), text: body.to_string(),
            }],
        };
        let resp = self.client.post(&self.cfg.webhook_url).json(&payload).send().await?;
        if !resp.status().is_success() {
            let code = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(ChannelError::Upstream(code, body));
        }
        Ok(())
    }
}
