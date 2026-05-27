//! Free Mobile (FR) — GET https://smsapi.free-mobile.fr/sendmsg
//! ?user=...&pass=...&msg=... Only delivers to your own number.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct FreeMobileConfig {
    pub user: String,
    pub pass: String,
}

pub struct Freemobile { cfg: FreeMobileConfig, client: reqwest::Client }

impl Freemobile {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: FreeMobileConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.user.is_empty() || cfg.pass.is_empty() {
            return Err(ChannelError::BadConfig("user + pass required".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[async_trait]
impl Channel for Freemobile {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let msg = format!("{subject}\n{body}");
        let resp = self.client.get("https://smsapi.free-mobile.fr/sendmsg")
            .query(&[
                ("user", self.cfg.user.as_str()),
                ("pass", self.cfg.pass.as_str()),
                ("msg",  msg.as_str()),
            ])
            .send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }
}
