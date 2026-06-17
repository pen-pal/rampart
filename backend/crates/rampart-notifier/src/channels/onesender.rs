//! Onesender — POST <base_url>/api/v1/send-message?api_token=... with
//! JSON {recipient, type, message}. Self-hosted WhatsApp gateway.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct OnesenderConfig {
    pub base_url: String,
    pub api_token: String,
    /// E.164 (no '+') or group jid
    pub recipient: String,
}

pub struct Onesender {
    cfg: OnesenderConfig,
    client: reqwest::Client,
}

impl Onesender {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: OnesenderConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.api_token.is_empty() || cfg.recipient.is_empty() {
            return Err(ChannelError::BadConfig(
                "api_token + recipient required".into(),
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
    recipient: &'a str,
    #[serde(rename = "type")]
    kind: &'static str,
    message: Msg,
}
#[derive(Serialize)]
struct Msg {
    text: String,
}

#[async_trait]
impl Channel for Onesender {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let url = format!(
            "{}/api/v1/send-message",
            self.cfg.base_url.trim_end_matches('/')
        );
        let payload = Payload {
            recipient: &self.cfg.recipient,
            kind: "text",
            message: Msg {
                text: format!("{subject}\n{body}"),
            },
        };
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.cfg.api_token)
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
