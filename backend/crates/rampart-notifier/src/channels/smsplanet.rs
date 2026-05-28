//! SMSPlanet.pl — POST /sms with bearer key.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct SmsPlanetConfig {
    pub api_key: String,
    pub sender: String,
    pub to: String,
}

pub struct Smsplanet {
    cfg: SmsPlanetConfig,
    client: reqwest::Client,
}

impl Smsplanet {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: SmsPlanetConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.api_key.is_empty() || cfg.to.is_empty() {
            return Err(ChannelError::BadConfig("api_key + to required".into()));
        }
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }
}

#[async_trait]
impl Channel for Smsplanet {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let text = format!("{subject}\n{body}");
        let resp = self
            .client
            .post("https://api2.smsplanet.pl/sms")
            .bearer_auth(&self.cfg.api_key)
            .form(&[
                ("from", self.cfg.sender.as_str()),
                ("to", self.cfg.to.as_str()),
                ("msg", text.as_str()),
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
