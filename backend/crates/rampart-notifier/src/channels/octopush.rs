//! Octopush — POST https://api.octopush.com/v1/public/sms-campaign/send
//! with API login + API key headers.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct OctopushConfig {
    pub api_login: String,
    pub api_key:   String,
    pub sender:    String,
    /// comma-separated E.164
    pub to:        String,
}

pub struct Octopush { cfg: OctopushConfig, client: reqwest::Client }

impl Octopush {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: OctopushConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.api_key.is_empty() || cfg.to.is_empty() {
            return Err(ChannelError::BadConfig("api_key + to required".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    text:       String,
    recipients: Vec<Recipient<'a>>,
    #[serde(rename = "type")]
    kind:       &'static str,
    sender:     &'a str,
    purpose:    &'static str,
}
#[derive(Serialize)]
struct Recipient<'a> { phone_number: &'a str }

#[async_trait]
impl Channel for Octopush {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let payload = Payload {
            text: format!("{subject}\n{body}"),
            recipients: self.cfg.to.split(',').map(str::trim).filter(|s| !s.is_empty())
                .map(|n| Recipient { phone_number: n }).collect(),
            kind: "sms_premium",
            sender: &self.cfg.sender,
            purpose: "alerting",
        };
        let resp = self.client.post("https://api.octopush.com/v1/public/sms-campaign/send")
            .header("api-login", &self.cfg.api_login)
            .header("api-key",   &self.cfg.api_key)
            .json(&payload).send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }
}
