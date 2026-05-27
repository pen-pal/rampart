//! MAX Messenger (RU) — POST https://botapi.max.ru/messages?access_token=...&chat_id=...

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct MaxConfig {
    pub access_token: String,
    pub chat_id:      String,
}

pub struct MaxMessenger { cfg: MaxConfig, client: reqwest::Client }

impl MaxMessenger {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: MaxConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.access_token.is_empty() || cfg.chat_id.is_empty() {
            return Err(ChannelError::BadConfig("access_token + chat_id required".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[derive(Serialize)]
struct Payload { text: String }

#[async_trait]
impl Channel for MaxMessenger {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let url = format!(
            "https://botapi.max.ru/messages?access_token={}&chat_id={}",
            self.cfg.access_token, self.cfg.chat_id,
        );
        let resp = self.client.post(&url)
            .json(&Payload { text: format!("{subject}\n{body}") })
            .send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }
}
