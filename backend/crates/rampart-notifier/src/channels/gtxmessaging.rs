//! GtxMessaging — POST https://srv2.gtx-messaging.net/api/sms/<api_key>/<sender_id>

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct GtxConfig {
    pub api_key: String,
    pub sender_id: String,
    /// comma-separated E.164
    pub to: String,
}

pub struct Gtxmessaging {
    cfg: GtxConfig,
    client: reqwest::Client,
}

impl Gtxmessaging {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: GtxConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.api_key.is_empty() || cfg.sender_id.is_empty() || cfg.to.is_empty() {
            return Err(ChannelError::BadConfig(
                "api_key + sender_id + to required".into(),
            ));
        }
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }
}

#[async_trait]
impl Channel for Gtxmessaging {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let url = format!(
            "https://srv2.gtx-messaging.net/api/sms/{}/{}",
            self.cfg.api_key, self.cfg.sender_id,
        );
        let resp = self
            .client
            .post(&url)
            .form(&[
                ("to", self.cfg.to.as_str()),
                ("message", format!("{subject}\n{body}").as_str()),
            ])
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
