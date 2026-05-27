//! PushDeer — pushdeer.com (or self-hosted) lightweight push.
//! Endpoint: <server>/message/push?pushkey=<key>&text=<text>&desp=<desp>

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct PushdeerConfig {
    pub push_key: String,
    #[serde(default = "default_server")]
    pub server:   String,
}
fn default_server() -> String { "https://api2.pushdeer.com".into() }

pub struct Pushdeer { cfg: PushdeerConfig, client: reqwest::Client }

impl Pushdeer {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: PushdeerConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.push_key.trim().is_empty() {
            return Err(ChannelError::BadConfig("push_key required".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[async_trait]
impl Channel for Pushdeer {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let url = format!("{}/message/push", self.cfg.server.trim_end_matches('/'));
        let resp = self.client.post(&url)
            .form(&[
                ("pushkey", self.cfg.push_key.as_str()),
                ("text",    subject),
                ("desp",    body),
                ("type",    "markdown"),
            ])
            .send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }
}
