//! Cisco Webex — POST /v1/messages with bearer auth + roomId.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct WebexConfig {
    pub bot_token: String,
    pub room_id: String,
}

pub struct Webex {
    cfg: WebexConfig,
    client: reqwest::Client,
}

impl Webex {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: WebexConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.bot_token.is_empty() || cfg.room_id.is_empty() {
            return Err(ChannelError::BadConfig(
                "bot_token + room_id required".into(),
            ));
        }
        Ok(Self {
            cfg,
            client: crate::http::client(),
        })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    #[serde(rename = "roomId")]
    room_id: &'a str,
    markdown: String,
}

#[async_trait]
impl Channel for Webex {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let payload = Payload {
            room_id: &self.cfg.room_id,
            markdown: format!("**{subject}**\n{body}"),
        };
        let resp = self
            .client
            .post("https://webexapis.com/v1/messages")
            .bearer_auth(&self.cfg.bot_token)
            .json(&payload)
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
