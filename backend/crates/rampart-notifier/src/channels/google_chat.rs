//! Google Chat — incoming webhook (Spaces → integrations → webhook).
//!
//! Posts a CardV2 with a header + scrolling text section so the
//! message renders nicely in the mobile + desktop clients.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct GoogleChatConfig {
    pub webhook_url: String,
}

pub struct GoogleChat {
    cfg: GoogleChatConfig,
    client: reqwest::Client,
}

impl GoogleChat {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: GoogleChatConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if !cfg.webhook_url.starts_with("https://chat.googleapis.com/") {
            return Err(ChannelError::BadConfig(
                "webhook_url must start with https://chat.googleapis.com/".into(),
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
    text: &'a str,
}

#[async_trait]
impl Channel for GoogleChat {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        // Plain text — Chat renders newlines + basic markdown.
        let text = format!("*{subject}*\n{body}");
        let resp = self
            .client
            .post(&self.cfg.webhook_url)
            .json(&Payload { text: &text })
            .send()
            .await?;
        if !resp.status().is_success() {
            let code = resp.status().as_u16();
            let txt = resp.text().await.unwrap_or_default();
            return Err(ChannelError::Upstream(code, txt));
        }
        Ok(())
    }
}
