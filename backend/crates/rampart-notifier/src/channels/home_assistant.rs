//! Home Assistant — REST /api/services/notify/<service> with bearer auth.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct HomeAssistantConfig {
    pub base_url: String,
    pub long_lived_token: String,
    /// e.g. "mobile_app_<device>" or "persistent_notification".
    pub notify_service: String,
}

pub struct HomeAssistant {
    cfg: HomeAssistantConfig,
    client: reqwest::Client,
}

impl HomeAssistant {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: HomeAssistantConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.long_lived_token.trim().is_empty() || cfg.notify_service.trim().is_empty() {
            return Err(ChannelError::BadConfig(
                "long_lived_token and notify_service required".into(),
            ));
        }
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    title: &'a str,
    message: &'a str,
}

#[async_trait]
impl Channel for HomeAssistant {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let url = format!(
            "{}/api/services/notify/{}",
            self.cfg.base_url.trim_end_matches('/'),
            self.cfg.notify_service,
        );
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.cfg.long_lived_token)
            .json(&Payload {
                title: subject,
                message: body,
            })
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
