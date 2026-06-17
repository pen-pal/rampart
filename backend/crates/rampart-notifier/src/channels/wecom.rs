//! WeCom (企业微信) group bot.
//!
//! Endpoint: https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=<bot_key>
//! Payload: msgtype=text with optional mentioned_mobile_list.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct WecomConfig {
    pub bot_key: String,
    #[serde(default)]
    pub mentioned_mobile_list: Vec<String>,
}

pub struct Wecom {
    cfg: WecomConfig,
    client: reqwest::Client,
}

impl Wecom {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: WecomConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.bot_key.trim().is_empty() {
            return Err(ChannelError::BadConfig("bot_key required".into()));
        }
        Ok(Self {
            cfg,
            client: crate::http::client(),
        })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    msgtype: &'static str,
    text: TextBody<'a>,
}
#[derive(Serialize)]
struct TextBody<'a> {
    content: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    mentioned_mobile_list: &'a Vec<String>,
}

#[async_trait]
impl Channel for Wecom {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let url = format!(
            "https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key={}",
            self.cfg.bot_key,
        );
        let content = format!("{subject}\n{body}");
        let payload = Payload {
            msgtype: "text",
            text: TextBody {
                content,
                mentioned_mobile_list: &self.cfg.mentioned_mobile_list,
            },
        };
        let resp = self.client.post(&url).json(&payload).send().await?;
        // WeCom returns 200 OK with {"errcode":N,"errmsg":...} — N!=0 is failure.
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(ChannelError::Upstream(status.as_u16(), body));
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
            if v.get("errcode").and_then(|c| c.as_i64()) != Some(0) {
                return Err(ChannelError::Upstream(200, body));
            }
        }
        Ok(())
    }
}
