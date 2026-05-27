//! Pushcut (iOS automation) — POST https://api.pushcut.io/v1/notifications/<name>
//! with bearer api_key.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct PushcutConfig {
    pub api_key:           String,
    pub notification_name: String,
}

pub struct Pushcut { cfg: PushcutConfig, client: reqwest::Client }

impl Pushcut {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: PushcutConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.api_key.is_empty() || cfg.notification_name.is_empty() {
            return Err(ChannelError::BadConfig("api_key + notification_name required".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    title: &'a str,
    text:  &'a str,
}

#[async_trait]
impl Channel for Pushcut {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let url = format!("https://api.pushcut.io/v1/notifications/{}", self.cfg.notification_name);
        let resp = self.client.post(&url)
            .header("API-Key", &self.cfg.api_key)
            .json(&Payload { title: subject, text: body })
            .send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }
}
