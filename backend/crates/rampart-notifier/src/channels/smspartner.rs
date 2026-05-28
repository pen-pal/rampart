//! SMSPartner.fr — POST https://api.smspartner.fr/v1/send with api_key in body.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct SmsPartnerConfig {
    pub api_key: String,
    pub sender: String,
    /// comma-separated FR-format numbers
    pub to: String,
}

pub struct Smspartner {
    cfg: SmsPartnerConfig,
    client: reqwest::Client,
}

impl Smspartner {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: SmsPartnerConfig = serde_json::from_value(raw.clone())
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

#[derive(Serialize)]
struct Payload<'a> {
    #[serde(rename = "apiKey")]
    api_key: &'a str,
    #[serde(rename = "phoneNumbers")]
    phone_numbers: &'a str,
    message: String,
    #[serde(rename = "sender")]
    sender: &'a str,
}

#[async_trait]
impl Channel for Smspartner {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let payload = Payload {
            api_key: &self.cfg.api_key,
            phone_numbers: &self.cfg.to,
            message: format!("{subject}\n{body}"),
            sender: &self.cfg.sender,
        };
        let resp = self
            .client
            .post("https://api.smspartner.fr/v1/send")
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
