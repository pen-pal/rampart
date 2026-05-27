//! SpugPush (Spug推送助手) — GET https://push.spug.cc/send/<template>?name=...&content=...

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct SpugConfig {
    pub template_code: String,
}

pub struct SpugPush { cfg: SpugConfig, client: reqwest::Client }

impl SpugPush {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: SpugConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.template_code.is_empty() {
            return Err(ChannelError::BadConfig("template_code required".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[async_trait]
impl Channel for SpugPush {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let url = format!("https://push.spug.cc/send/{}", self.cfg.template_code);
        let resp = self.client.post(&url)
            .form(&[("title", subject), ("content", body)])
            .send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }
}
