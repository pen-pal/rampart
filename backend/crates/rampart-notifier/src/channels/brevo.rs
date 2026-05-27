//! Brevo (formerly Sendinblue) — transactional email via
//! api.brevo.com/v3/smtp/email.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct BrevoConfig {
    pub api_key:    String,
    pub from_email: String,
    #[serde(default)]
    pub from_name:  Option<String>,
    pub to_email:   String,
    #[serde(default)]
    pub to_name:    Option<String>,
}

pub struct Brevo { cfg: BrevoConfig, client: reqwest::Client }

impl Brevo {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: BrevoConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.api_key.trim().is_empty() {
            return Err(ChannelError::BadConfig("api_key required".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    sender:      Addr<'a>,
    to:          Vec<Addr<'a>>,
    subject:     &'a str,
    #[serde(rename = "textContent")]
    text:        &'a str,
}
#[derive(Serialize)]
struct Addr<'a> {
    email: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    name:  &'a Option<String>,
}

#[async_trait]
impl Channel for Brevo {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let payload = Payload {
            sender: Addr { email: &self.cfg.from_email, name: &self.cfg.from_name },
            to: vec![Addr { email: &self.cfg.to_email, name: &self.cfg.to_name }],
            subject,
            text: body,
        };
        let resp = self.client.post("https://api.brevo.com/v3/smtp/email")
            .header("api-key", &self.cfg.api_key)
            .json(&payload)
            .send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }
}
