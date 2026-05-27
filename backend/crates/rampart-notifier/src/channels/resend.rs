//! Resend — transactional email via api.resend.com/emails.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct ResendConfig {
    pub api_key: String,
    pub from:    String,
    pub to:      Recipients,
}

#[derive(Debug)]
pub struct Recipients(pub Vec<String>);
impl<'de> Deserialize<'de> for Recipients {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        if let Some(s) = v.as_str() {
            return Ok(Recipients(s.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()));
        }
        if let Some(arr) = v.as_array() {
            return Ok(Recipients(arr.iter().filter_map(|x| x.as_str().map(String::from)).collect()));
        }
        Err(serde::de::Error::custom("to must be string or array"))
    }
}

pub struct Resend { cfg: ResendConfig, client: reqwest::Client }

impl Resend {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: ResendConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.api_key.trim().is_empty() || cfg.to.0.is_empty() {
            return Err(ChannelError::BadConfig("api_key and to required".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    from:    &'a str,
    to:      &'a Vec<String>,
    subject: &'a str,
    text:    &'a str,
}

#[async_trait]
impl Channel for Resend {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let payload = Payload { from: &self.cfg.from, to: &self.cfg.to.0, subject, text: body };
        let resp = self.client.post("https://api.resend.com/emails")
            .bearer_auth(&self.cfg.api_key)
            .json(&payload)
            .send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }
}
