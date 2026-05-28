//! Pushbullet — push notifications via api.pushbullet.com.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct PushbulletConfig {
    pub access_token: String,
    #[serde(default)]
    pub device_iden: Option<String>,
}

pub struct Pushbullet {
    cfg: PushbulletConfig,
    client: reqwest::Client,
}

impl Pushbullet {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: PushbulletConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.access_token.trim().is_empty() {
            return Err(ChannelError::BadConfig("access_token required".into()));
        }
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    title: &'a str,
    body: &'a str,
    #[serde(rename = "device_iden", skip_serializing_if = "Option::is_none")]
    device: &'a Option<String>,
}

#[async_trait]
impl Channel for Pushbullet {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let payload = Payload {
            kind: "note",
            title: subject,
            body,
            device: &self.cfg.device_iden,
        };
        let resp = self
            .client
            .post("https://api.pushbullet.com/v2/pushes")
            .header("Access-Token", &self.cfg.access_token)
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
