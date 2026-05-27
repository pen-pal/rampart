//! 46elks SMS — POST https://api.46elks.com/a1/SMS, basic auth.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Sms46elksConfig {
    pub api_username: String,
    pub api_password: String,
    pub from:         String,
    /// comma-separated E.164 list
    pub to:           String,
}

pub struct Sms46elks { cfg: Sms46elksConfig, client: reqwest::Client }

impl Sms46elks {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: Sms46elksConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.api_username.is_empty() || cfg.api_password.is_empty() || cfg.to.is_empty() {
            return Err(ChannelError::BadConfig("missing required fields".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[async_trait]
impl Channel for Sms46elks {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let text = format!("{subject}\n{body}");
        for to in self.cfg.to.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let resp = self.client.post("https://api.46elks.com/a1/SMS")
                .basic_auth(&self.cfg.api_username, Some(&self.cfg.api_password))
                .form(&[("from", self.cfg.from.as_str()), ("to", to), ("message", text.as_str())])
                .send().await?;
            if !resp.status().is_success() {
                return Err(ChannelError::Upstream(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
            }
        }
        Ok(())
    }
}
