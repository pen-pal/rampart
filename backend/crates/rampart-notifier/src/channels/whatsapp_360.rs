//! WhatsApp via 360messenger — POST /api/sendMessage with API key.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct W360Config {
    pub api_key: String,
    /// E.164 phone number (no '+' or '@')
    pub phone: String,
}

pub struct Whatsapp360 {
    cfg: W360Config,
    client: reqwest::Client,
}

impl Whatsapp360 {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: W360Config = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.api_key.is_empty() || cfg.phone.is_empty() {
            return Err(ChannelError::BadConfig("api_key + phone required".into()));
        }
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    phone: &'a str,
    message: String,
}

#[async_trait]
impl Channel for Whatsapp360 {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let payload = Payload {
            phone: &self.cfg.phone,
            message: format!("{subject}\n{body}"),
        };
        let resp = self
            .client
            .post("https://api.360messenger.com/sendMessage")
            .header("Authorization", format!("Bearer {}", self.cfg.api_key))
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
