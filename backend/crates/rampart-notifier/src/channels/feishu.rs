//! Feishu (飞书) custom bot.
//!
//! Endpoint: https://open.feishu.cn/open-apis/bot/v2/hook/<token>
//! Simple text payload — `msg_type=text`, `content.text`.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct FeishuConfig {
    pub webhook_url: String,
}

pub struct Feishu {
    cfg: FeishuConfig,
    client: reqwest::Client,
}

impl Feishu {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: FeishuConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if !cfg.webhook_url.starts_with("https://open.feishu.cn/") {
            return Err(ChannelError::BadConfig(
                "webhook_url must start with https://open.feishu.cn/".into(),
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
    msg_type: &'static str,
    content: Content<'a>,
}
#[derive(Serialize)]
struct Content<'a> {
    text: &'a str,
}

#[async_trait]
impl Channel for Feishu {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let text = format!("{subject}\n{body}");
        let payload = Payload {
            msg_type: "text",
            content: Content { text: &text },
        };
        let resp = self
            .client
            .post(&self.cfg.webhook_url)
            .json(&payload)
            .send()
            .await?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(ChannelError::Upstream(status.as_u16(), body));
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
            if v.get("code").and_then(|c| c.as_i64()).unwrap_or(0) != 0 {
                return Err(ChannelError::Upstream(200, body));
            }
        }
        Ok(())
    }
}
