//! Pushy.me — POST https://api.pushy.me/push?api_key=<key>.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct PushyConfig {
    pub api_key: String,
    /// Pushy device token(s).
    pub to:      Vec<String>,
}

pub struct Pushy { cfg: PushyConfig, client: reqwest::Client }

impl Pushy {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: PushyConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.to.is_empty() {
            return Err(ChannelError::BadConfig("to required".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    to:           &'a Vec<String>,
    notification: Notif<'a>,
    data:         Data<'a>,
}
#[derive(Serialize)]
struct Notif<'a> { title: &'a str, body: &'a str }
#[derive(Serialize)]
struct Data<'a>  { message: &'a str }

#[async_trait]
impl Channel for Pushy {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let url = format!("https://api.pushy.me/push?api_key={}", self.cfg.api_key);
        let payload = Payload {
            to: &self.cfg.to,
            notification: Notif { title: subject, body },
            data: Data { message: body },
        };
        let resp = self.client.post(&url).json(&payload).send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }
}
