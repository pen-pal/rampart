//! WhatsApp via whapi.cloud — POST /messages/text with bearer token.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct WhapiConfig {
    pub api_token: String,
    /// "<phone>@s.whatsapp.net" or group jid.
    pub to: String,
    #[serde(default = "default_base")]
    pub base_url: String,
}
fn default_base() -> String {
    "https://gate.whapi.cloud".into()
}

pub struct WhatsappWhapi {
    cfg: WhapiConfig,
    client: reqwest::Client,
}

impl WhatsappWhapi {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: WhapiConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.api_token.trim().is_empty() || cfg.to.trim().is_empty() {
            return Err(ChannelError::BadConfig("api_token + to required".into()));
        }
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    to: &'a str,
    body: String,
}

#[async_trait]
impl Channel for WhatsappWhapi {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let url = format!("{}/messages/text", self.cfg.base_url.trim_end_matches('/'));
        let payload = Payload {
            to: &self.cfg.to,
            body: format!("{subject}\n{body}"),
        };
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.cfg.api_token)
            .json(&payload)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(
                resp.status().as_u16(),
                resp.text().await.unwrap_or_default(),
            ));
        }
        Ok(())
    }
}
