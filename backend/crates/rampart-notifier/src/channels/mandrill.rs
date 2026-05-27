//! Mailchimp Transactional (Mandrill) — POST /messages/send.json.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct MandrillConfig {
    pub api_key:    String,
    pub from_email: String,
    #[serde(default)]
    pub from_name:  Option<String>,
    pub to_email:   String,
}

pub struct Mandrill { cfg: MandrillConfig, client: reqwest::Client }

impl Mandrill {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: MandrillConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.api_key.is_empty() || cfg.to_email.is_empty() {
            return Err(ChannelError::BadConfig("api_key + to_email required".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[derive(Serialize)]
struct Wrap<'a> { key: &'a str, message: Msg<'a> }
#[derive(Serialize)]
struct Msg<'a> {
    from_email: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    from_name:  &'a Option<String>,
    to:         Vec<To<'a>>,
    subject:    &'a str,
    text:       &'a str,
}
#[derive(Serialize)]
struct To<'a> { email: &'a str, #[serde(rename = "type")] kind: &'static str }

#[async_trait]
impl Channel for Mandrill {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let payload = Wrap {
            key: &self.cfg.api_key,
            message: Msg {
                from_email: &self.cfg.from_email,
                from_name:  &self.cfg.from_name,
                to: vec![To { email: &self.cfg.to_email, kind: "to" }],
                subject,
                text: body,
            },
        };
        let resp = self.client.post("https://mandrillapp.com/api/1.0/messages/send.json")
            .json(&payload).send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }
}
