//! Postmark — POST /email with X-Postmark-Server-Token.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct PostmarkConfig {
    pub server_token: String,
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub message_stream: Option<String>,
}

pub struct Postmark {
    cfg: PostmarkConfig,
    client: reqwest::Client,
}

impl Postmark {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: PostmarkConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.server_token.is_empty() || cfg.to.is_empty() {
            return Err(ChannelError::BadConfig("server_token + to required".into()));
        }
        Ok(Self {
            cfg,
            client: crate::http::client(),
        })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    #[serde(rename = "From")]
    from: &'a str,
    #[serde(rename = "To")]
    to: &'a str,
    #[serde(rename = "Subject")]
    subject: &'a str,
    #[serde(rename = "TextBody")]
    text_body: &'a str,
    #[serde(rename = "MessageStream", skip_serializing_if = "Option::is_none")]
    message_stream: &'a Option<String>,
}

#[async_trait]
impl Channel for Postmark {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let payload = Payload {
            from: &self.cfg.from,
            to: &self.cfg.to,
            subject,
            text_body: body,
            message_stream: &self.cfg.message_stream,
        };
        let resp = self
            .client
            .post("https://api.postmarkapp.com/email")
            .header("Accept", "application/json")
            .header("X-Postmark-Server-Token", &self.cfg.server_token)
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
