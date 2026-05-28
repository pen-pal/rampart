//! WPush.cn — POST https://api.wpush.cn/api/v1/send/<channel>?token=...

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct WpushConfig {
    pub api_key: String,
    /// comma-separated channel IDs; e.g. wechat, email, sms, dingtalk.
    pub channel: String,
}

pub struct Wpush {
    cfg: WpushConfig,
    client: reqwest::Client,
}

impl Wpush {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: WpushConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.api_key.is_empty() || cfg.channel.is_empty() {
            return Err(ChannelError::BadConfig("api_key + channel required".into()));
        }
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    api_key: &'a str,
    title: &'a str,
    content: &'a str,
    channel: &'a str,
}

#[async_trait]
impl Channel for Wpush {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let payload = Payload {
            api_key: &self.cfg.api_key,
            title: subject,
            content: body,
            channel: &self.cfg.channel,
        };
        let resp = self
            .client
            .post("https://api.wpush.cn/api/v1/send")
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
