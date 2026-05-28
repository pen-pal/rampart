//! Telnyx SMS — POST /v2/messages with bearer auth.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct TelnyxConfig {
    pub api_key: String,
    pub from: String,
    /// comma-separated E.164 numbers
    pub to: String,
}

pub struct Telnyx {
    cfg: TelnyxConfig,
    client: reqwest::Client,
}

impl Telnyx {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: TelnyxConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.api_key.is_empty() || cfg.to.is_empty() {
            return Err(ChannelError::BadConfig("api_key and to required".into()));
        }
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    from: &'a str,
    to: &'a str,
    text: String,
}

#[async_trait]
impl Channel for Telnyx {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let text = format!("{subject}\n{body}");
        for to in self
            .cfg
            .to
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let payload = Payload {
                from: &self.cfg.from,
                to,
                text: text.clone(),
            };
            let resp = self
                .client
                .post("https://api.telnyx.com/v2/messages")
                .bearer_auth(&self.cfg.api_key)
                .json(&payload)
                .send()
                .await?;
            if !resp.status().is_success() {
                return Err(ChannelError::Upstream(
                    resp.status().as_u16(),
                    resp.text().await.unwrap_or_default(),
                ));
            }
        }
        Ok(())
    }
}
