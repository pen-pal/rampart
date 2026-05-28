//! Stackfield — incoming webhook to a room.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct StackfieldConfig {
    pub webhook_url: String,
}

pub struct Stackfield {
    cfg: StackfieldConfig,
    client: reqwest::Client,
}

impl Stackfield {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: StackfieldConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if !cfg.webhook_url.starts_with("https://") {
            return Err(ChannelError::BadConfig("webhook_url required".into()));
        }
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }
}

#[derive(Serialize)]
struct Payload {
    message_text: String,
}

#[async_trait]
impl Channel for Stackfield {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let resp = self
            .client
            .post(&self.cfg.webhook_url)
            .json(&Payload {
                message_text: format!("**{subject}**\n{body}"),
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
