//! PushPlus (推送加) — pushplus.plus push to WeChat via token.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct PushplusConfig {
    pub token: String,
    #[serde(default)]
    pub topic: Option<String>,
}

pub struct Pushplus {
    cfg: PushplusConfig,
    client: reqwest::Client,
}

impl Pushplus {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: PushplusConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.token.trim().is_empty() {
            return Err(ChannelError::BadConfig("token required".into()));
        }
        Ok(Self {
            cfg,
            client: crate::http::client(),
        })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    token: &'a str,
    title: &'a str,
    content: &'a str,
    template: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    topic: &'a Option<String>,
}

#[async_trait]
impl Channel for Pushplus {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let payload = Payload {
            token: &self.cfg.token,
            title: subject,
            content: body,
            template: "txt",
            topic: &self.cfg.topic,
        };
        let resp = self
            .client
            .post("https://www.pushplus.plus/send")
            .json(&payload)
            .send()
            .await?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(ChannelError::Upstream(status.as_u16(), body));
        }
        // {"code": 200, ...} on success.
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
            if v.get("code").and_then(|c| c.as_i64()) != Some(200) {
                return Err(ChannelError::Upstream(200, body));
            }
        }
        Ok(())
    }
}
