//! OneChat (TH) — POST /api/v1/send-message with bearer + chat_id.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct OnechatConfig {
    pub bot_token: String,
    pub chat_id: String,
}

pub struct Onechat {
    cfg: OnechatConfig,
    client: reqwest::Client,
}

impl Onechat {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: OnechatConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.bot_token.is_empty() || cfg.chat_id.is_empty() {
            return Err(ChannelError::BadConfig(
                "bot_token + chat_id required".into(),
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
    to_user_id: &'a str,
    bot_id: &'a str,
    message: String,
    custom_notification: String,
}

#[async_trait]
impl Channel for Onechat {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let payload = Payload {
            to_user_id: &self.cfg.chat_id,
            bot_id: &self.cfg.bot_token,
            message: format!("{subject}\n{body}"),
            custom_notification: subject.into(),
        };
        let resp = self
            .client
            .post("https://chat-api.onechat.one/api/v1/push_message")
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
