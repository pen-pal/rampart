//! Notifery — POST event payload to notifery.com with api token.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct NotiferyConfig {
    pub api_token: String,
    /// group key for the event (Notifery dashboard groups by this).
    #[serde(default = "default_group")]
    pub group: String,
}
fn default_group() -> String { "rampart".into() }

pub struct Notifery { cfg: NotiferyConfig, client: reqwest::Client }

impl Notifery {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: NotiferyConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.api_token.trim().is_empty() {
            return Err(ChannelError::BadConfig("api_token required".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    title:       &'a str,
    description: &'a str,
    group:       &'a str,
}

#[async_trait]
impl Channel for Notifery {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let payload = Payload { title: subject, description: body, group: &self.cfg.group };
        let resp = self.client.post("https://api.notifery.com/event")
            .header("Authorization", &self.cfg.api_token)
            .json(&payload).send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }
}
