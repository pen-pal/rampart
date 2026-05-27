//! Mattermost incoming webhook.
//!
//! Mattermost's webhook payload is Slack-compatible (text + attachments
//! with a color attribute). Setup: in Mattermost, Main Menu → Integrations
//! → Incoming Webhooks → Add. Copy the URL.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct MattermostConfig {
    pub webhook_url: String,
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
}

#[derive(Debug)]
pub struct Mattermost {
    cfg: MattermostConfig,
    client: reqwest::Client,
}

impl Mattermost {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: MattermostConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if !cfg.webhook_url.starts_with("https://") && !cfg.webhook_url.starts_with("http://") {
            return Err(ChannelError::BadConfig(
                "webhook_url must start with http(s)://".into(),
            ));
        }
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }
}

#[derive(Serialize)]
struct MmPayload<'a> {
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    channel: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    username: &'a Option<String>,
    attachments: Vec<MmAttachment>,
}

#[derive(Serialize)]
struct MmAttachment {
    color: String,
    title: String,
    text: String,
}

#[async_trait]
impl Channel for Mattermost {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError> {
        let color = match event.heartbeat.status {
            rampart_core::MonitorStatus::Up => "#10b981",
            rampart_core::MonitorStatus::Down => "#ef4444",
            _ => "#f59e0b",
        }
        .to_string();
        let payload = MmPayload {
            text: subject.to_string(),
            channel: &self.cfg.channel,
            username: &self.cfg.username,
            attachments: vec![MmAttachment {
                color,
                title: subject.to_string(),
                text: body.to_string(),
            }],
        };
        let resp = self
            .client
            .post(&self.cfg.webhook_url)
            .json(&payload)
            .send()
            .await?;
        if !resp.status().is_success() {
            let code = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(ChannelError::Upstream(code, body));
        }
        Ok(())
    }
}
