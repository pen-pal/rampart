//! Lark (Feishu international) custom bot. Same payload as Feishu;
//! the only practical difference is the host (open.larksuite.com vs
//! open.feishu.cn) and that's user-configured in the webhook URL.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct LarkConfig {
    pub webhook_url: String,
}

pub struct Lark { cfg: LarkConfig, client: reqwest::Client }

impl Lark {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: LarkConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if !cfg.webhook_url.contains("larksuite.com") && !cfg.webhook_url.contains("feishu.cn") {
            return Err(ChannelError::BadConfig(
                "webhook_url must point at lark / feishu".into(),
            ));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    msg_type: &'static str,
    content: Content<'a>,
}
#[derive(Serialize)]
struct Content<'a> { text: &'a str }

#[async_trait]
impl Channel for Lark {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let text = format!("{subject}\n{body}");
        let resp = self.client.post(&self.cfg.webhook_url)
            .json(&Payload { msg_type: "text", content: Content { text: &text } })
            .send().await?;
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
