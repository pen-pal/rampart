//! VK — messages.send via the user/community access token.
//! https://api.vk.com/method/messages.send

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct VkConfig {
    pub access_token: String,
    pub peer_id: i64,
    #[serde(default = "default_api_version")]
    pub api_version: String,
}
fn default_api_version() -> String {
    "5.199".into()
}

pub struct Vk {
    cfg: VkConfig,
    client: reqwest::Client,
}

impl Vk {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: VkConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.access_token.is_empty() || cfg.peer_id == 0 {
            return Err(ChannelError::BadConfig(
                "access_token + peer_id required".into(),
            ));
        }
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }
}

#[async_trait]
impl Channel for Vk {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let text = format!("{subject}\n{body}");
        let random_id = (rand::random::<u32>()) as i64;
        let resp = self
            .client
            .post("https://api.vk.com/method/messages.send")
            .form(&[
                ("access_token", self.cfg.access_token.as_str()),
                ("v", self.cfg.api_version.as_str()),
                ("peer_id", self.cfg.peer_id.to_string().as_str()),
                ("random_id", random_id.to_string().as_str()),
                ("message", text.as_str()),
            ])
            .send()
            .await?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(ChannelError::Upstream(status.as_u16(), body));
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
            if v.get("error").is_some() {
                return Err(ChannelError::Upstream(200, body));
            }
        }
        Ok(())
    }
}
