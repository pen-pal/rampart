//! WhatsApp via WAHA — self-hosted gateway (github.com/devlikeapro/waha).
//! POST <base>/api/sendText with session + chatId + text.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct WahaConfig {
    pub base_url: String,
    pub session: String,
    /// "<phone>@c.us" for individual or group jid.
    pub chat_id: String,
    #[serde(default)]
    pub api_key: Option<String>,
}

pub struct WhatsappWaha {
    cfg: WahaConfig,
    client: reqwest::Client,
}

impl WhatsappWaha {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: WahaConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.chat_id.trim().is_empty() {
            return Err(ChannelError::BadConfig("chat_id required".into()));
        }
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    session: &'a str,
    #[serde(rename = "chatId")]
    chat_id: &'a str,
    text: String,
}

#[async_trait]
impl Channel for WhatsappWaha {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let url = format!("{}/api/sendText", self.cfg.base_url.trim_end_matches('/'));
        let mut req = self.client.post(&url).json(&Payload {
            session: &self.cfg.session,
            chat_id: &self.cfg.chat_id,
            text: format!("{subject}\n{body}"),
        });
        if let Some(k) = &self.cfg.api_key {
            req = req.header("X-Api-Key", k);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(
                resp.status().as_u16(),
                resp.text().await.unwrap_or_default(),
            ));
        }
        Ok(())
    }
}
