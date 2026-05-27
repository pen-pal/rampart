//! Bale Messenger — Telegram-style bot API. POST tapi.bale.ai/bot<token>/sendMessage.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct BaleConfig {
    pub bot_token: String,
    pub chat_id:   String,
}

pub struct Bale { cfg: BaleConfig, client: reqwest::Client }

impl Bale {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: BaleConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.bot_token.trim().is_empty() || cfg.chat_id.trim().is_empty() {
            return Err(ChannelError::BadConfig("bot_token + chat_id required".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    chat_id: &'a str,
    text:    String,
}

#[async_trait]
impl Channel for Bale {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let url = format!("https://tapi.bale.ai/bot{}/sendMessage", self.cfg.bot_token);
        let payload = Payload { chat_id: &self.cfg.chat_id, text: format!("{subject}\n{body}") };
        let resp = self.client.post(&url).json(&payload).send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }
}
