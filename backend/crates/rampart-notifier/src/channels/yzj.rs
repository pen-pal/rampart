//! YZJ (云之家) custom robot — POST the user-supplied webhook URL.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct YzjConfig {
    pub webhook_url: String,
}

pub struct Yzj { cfg: YzjConfig, client: reqwest::Client }

impl Yzj {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: YzjConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if !cfg.webhook_url.starts_with("http") {
            return Err(ChannelError::BadConfig("webhook_url required".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[derive(Serialize)]
struct Payload {
    content: String,
}

#[async_trait]
impl Channel for Yzj {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let resp = self.client.post(&self.cfg.webhook_url)
            .json(&Payload { content: format!("{subject}\n{body}") })
            .send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }
}
