//! Gorush — self-hosted push relay for FCM/APNs. POST /api/push.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct GorushConfig {
    pub server:   String,
    /// "ios" or "android"
    pub platform: String,
    /// Device tokens
    pub tokens:   Vec<String>,
    /// FCM topic if you'd rather broadcast.
    #[serde(default)]
    pub topic:    Option<String>,
}

pub struct Gorush { cfg: GorushConfig, client: reqwest::Client }

impl Gorush {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: GorushConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if !cfg.server.starts_with("http") || cfg.tokens.is_empty() {
            return Err(ChannelError::BadConfig("server + tokens required".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[derive(Serialize)]
struct Wrapper<'a> { notifications: Vec<Push<'a>> }
#[derive(Serialize)]
struct Push<'a> {
    tokens:   &'a Vec<String>,
    platform: u8,
    title:    &'a str,
    message:  &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    topic:    &'a Option<String>,
}

#[async_trait]
impl Channel for Gorush {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let platform = if self.cfg.platform == "ios" { 1 } else { 2 };
        let push = Push {
            tokens: &self.cfg.tokens,
            platform,
            title: subject,
            message: body,
            topic: &self.cfg.topic,
        };
        let url = format!("{}/api/push", self.cfg.server.trim_end_matches('/'));
        let resp = self.client.post(&url)
            .json(&Wrapper { notifications: vec![push] })
            .send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }
}
