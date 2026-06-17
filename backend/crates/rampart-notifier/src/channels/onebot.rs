//! OneBot v11 — QQ-bot framework. POST <http_url>/send_private_msg
//! or /send_group_msg with {user_id|group_id, message}.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct OnebotConfig {
    pub http_url: String,
    /// "group" | "private"; default group.
    #[serde(default = "default_kind")]
    pub kind: String,
    pub target_id: i64,
    #[serde(default)]
    pub access_token: Option<String>,
}
fn default_kind() -> String {
    "group".into()
}

pub struct Onebot {
    cfg: OnebotConfig,
    client: reqwest::Client,
}

impl Onebot {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: OnebotConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.target_id == 0 || !cfg.http_url.starts_with("http") {
            return Err(ChannelError::BadConfig(
                "http_url + target_id required".into(),
            ));
        }
        Ok(Self {
            cfg,
            client: crate::http::client(),
        })
    }
}

#[derive(Serialize)]
struct GroupPayload<'a> {
    group_id: i64,
    message: &'a str,
}
#[derive(Serialize)]
struct PrivatePayload<'a> {
    user_id: i64,
    message: &'a str,
}

#[async_trait]
impl Channel for Onebot {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let text = format!("{subject}\n{body}");
        let (path, payload) = if self.cfg.kind == "private" {
            (
                "send_private_msg",
                serde_json::to_value(PrivatePayload {
                    user_id: self.cfg.target_id,
                    message: &text,
                })
                .unwrap(),
            )
        } else {
            (
                "send_group_msg",
                serde_json::to_value(GroupPayload {
                    group_id: self.cfg.target_id,
                    message: &text,
                })
                .unwrap(),
            )
        };
        let url = format!("{}/{}", self.cfg.http_url.trim_end_matches('/'), path);
        let mut req = self.client.post(&url).json(&payload);
        if let Some(t) = &self.cfg.access_token {
            req = req.bearer_auth(t);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(
                resp.status().as_u16(),
                resp.text().await.unwrap_or_default(),
            ));
        }
        Ok(())
    }
}
