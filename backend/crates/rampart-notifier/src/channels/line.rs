//! LINE Messaging API — push message.
//!
//! Needs a channel access token (from the LINE Developers Console) and
//! the destination user / group / room id. This targets the modern
//! Messaging API, not the deprecated LINE Notify service.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct LineConfig {
    pub channel_access_token: String,
    pub to: String,
}

pub struct Line {
    cfg: LineConfig,
    client: reqwest::Client,
}

impl Line {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: LineConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.channel_access_token.trim().is_empty() || cfg.to.trim().is_empty() {
            return Err(ChannelError::BadConfig(
                "channel_access_token and to are required".into(),
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
    to: &'a str,
    messages: Vec<Msg>,
}
#[derive(Serialize)]
struct Msg {
    #[serde(rename = "type")]
    kind: &'static str,
    text: String,
}

#[async_trait]
impl Channel for Line {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let text = format!("{subject}\n{body}");
        // LINE caps a single text message at 5000 chars.
        let text: String = text.chars().take(5000).collect();
        let payload = Payload {
            to: &self.cfg.to,
            messages: vec![Msg { kind: "text", text }],
        };
        let resp = self
            .client
            .post("https://api.line.me/v2/bot/message/push")
            .bearer_auth(&self.cfg.channel_access_token)
            .json(&payload)
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
