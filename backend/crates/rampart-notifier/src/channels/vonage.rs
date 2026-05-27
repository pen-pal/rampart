//! Vonage (Nexmo) SMS — POST https://rest.nexmo.com/sms/json.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct VonageConfig {
    pub api_key:    String,
    pub api_secret: String,
    pub from:       String,
    /// E.164 number (no '+'); SMS-only, no comma-separated multi.
    pub to:         String,
}

pub struct Vonage { cfg: VonageConfig, client: reqwest::Client }

impl Vonage {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: VonageConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.api_key.is_empty() || cfg.api_secret.is_empty() || cfg.to.is_empty() {
            return Err(ChannelError::BadConfig("missing required fields".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[async_trait]
impl Channel for Vonage {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let text = format!("{subject}\n{body}");
        let resp = self.client.post("https://rest.nexmo.com/sms/json")
            .form(&[
                ("api_key",    self.cfg.api_key.as_str()),
                ("api_secret", self.cfg.api_secret.as_str()),
                ("from",       self.cfg.from.as_str()),
                ("to",         self.cfg.to.as_str()),
                ("text",       text.as_str()),
            ])
            .send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }
}
