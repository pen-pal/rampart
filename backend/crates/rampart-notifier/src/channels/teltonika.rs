//! Teltonika RUT SMS Gateway — POST <router>/cgi-bin/sms_send.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct TeltonikaConfig {
    pub base_url: String,
    pub username: String,
    pub password: String,
    pub number:   String,
}

pub struct Teltonika { cfg: TeltonikaConfig, client: reqwest::Client }

impl Teltonika {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: TeltonikaConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.username.is_empty() || cfg.password.is_empty() || cfg.number.is_empty() {
            return Err(ChannelError::BadConfig("missing required fields".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[async_trait]
impl Channel for Teltonika {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let url = format!("{}/cgi-bin/sms_send", self.cfg.base_url.trim_end_matches('/'));
        let text = format!("{subject}\n{body}");
        let resp = self.client.get(&url)
            .query(&[
                ("username", self.cfg.username.as_str()),
                ("password", self.cfg.password.as_str()),
                ("number",   self.cfg.number.as_str()),
                ("text",     text.as_str()),
            ])
            .send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }
}
