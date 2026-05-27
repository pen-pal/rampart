//! ClickUp — POST /api/v2/list/<list_id>/task with API token.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct ClickupConfig {
    pub api_token: String,
    pub list_id:   String,
}

pub struct Clickup { cfg: ClickupConfig, client: reqwest::Client }

impl Clickup {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: ClickupConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.api_token.is_empty() || cfg.list_id.is_empty() {
            return Err(ChannelError::BadConfig("api_token + list_id required".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    name:        &'a str,
    description: &'a str,
}

#[async_trait]
impl Channel for Clickup {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let url = format!("https://api.clickup.com/api/v2/list/{}/task", self.cfg.list_id);
        let resp = self.client.post(&url)
            .header("Authorization", &self.cfg.api_token)
            .json(&Payload { name: subject, description: body })
            .send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }
}
