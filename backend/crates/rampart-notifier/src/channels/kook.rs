//! Kook (formerly Kaiheila) — POST /api/v3/message/create with bot token.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct KookConfig {
    pub bot_token: String,
    /// "GROUP" or "PERSON"; default GROUP.
    #[serde(default = "default_target")]
    pub target_type: String,
    pub target_id:   String,
}
fn default_target() -> String { "GROUP".into() }

pub struct Kook { cfg: KookConfig, client: reqwest::Client }

impl Kook {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: KookConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.bot_token.is_empty() || cfg.target_id.is_empty() {
            return Err(ChannelError::BadConfig("bot_token + target_id required".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    #[serde(rename = "type")]
    kind:        u8,
    target_id:   &'a str,
    content:     String,
}

#[async_trait]
impl Channel for Kook {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        // type=1 plain text
        let payload = Payload { kind: 1, target_id: &self.cfg.target_id, content: format!("{subject}\n{body}") };
        let url = if self.cfg.target_type == "PERSON" {
            "https://www.kookapp.cn/api/v3/direct-message/create"
        } else {
            "https://www.kookapp.cn/api/v3/message/create"
        };
        let resp = self.client.post(url)
            .header("Authorization", format!("Bot {}", self.cfg.bot_token))
            .json(&payload).send().await?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(ChannelError::Upstream(status.as_u16(), body));
        }
        // Kook returns {"code":0,...} on success.
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
            if v.get("code").and_then(|c| c.as_i64()) != Some(0) {
                return Err(ChannelError::Upstream(200, body));
            }
        }
        Ok(())
    }
}
