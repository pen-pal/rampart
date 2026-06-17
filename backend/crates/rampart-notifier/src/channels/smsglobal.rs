//! SMSGlobal — POST /v2/sms with basic auth (api_key + api_secret).

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct SmsglobalConfig {
    pub api_key: String,
    pub api_secret: String,
    pub origin: String,
    /// comma-separated E.164 numbers
    pub destination: String,
}

pub struct Smsglobal {
    cfg: SmsglobalConfig,
    client: reqwest::Client,
}

impl Smsglobal {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: SmsglobalConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.api_key.is_empty() || cfg.api_secret.is_empty() || cfg.destination.is_empty() {
            return Err(ChannelError::BadConfig("missing required fields".into()));
        }
        Ok(Self {
            cfg,
            client: crate::http::client(),
        })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    origin: &'a str,
    destination: Vec<&'a str>,
    message: String,
}

#[async_trait]
impl Channel for Smsglobal {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let payload = Payload {
            origin: &self.cfg.origin,
            destination: self
                .cfg
                .destination
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect(),
            message: format!("{subject}\n{body}"),
        };
        let resp = self
            .client
            .post("https://api.smsglobal.com/v2/sms")
            .basic_auth(&self.cfg.api_key, Some(&self.cfg.api_secret))
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
