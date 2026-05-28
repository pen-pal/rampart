//! SMSEagle — self-hosted GSM gateway. v2 REST: POST /api/v2/messages/sms.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct SmsEagleConfig {
    pub base_url: String,
    pub access_token: String,
    /// comma-separated E.164
    pub to: String,
}

pub struct SmsEagle {
    cfg: SmsEagleConfig,
    client: reqwest::Client,
}

impl SmsEagle {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: SmsEagleConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if !cfg.base_url.starts_with("http") || cfg.access_token.is_empty() {
            return Err(ChannelError::BadConfig(
                "base_url + access_token required".into(),
            ));
        }
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    to: Vec<&'a str>,
    text: String,
    encoding: &'static str,
}

#[async_trait]
impl Channel for SmsEagle {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let url = format!(
            "{}/api/v2/messages/sms",
            self.cfg.base_url.trim_end_matches('/')
        );
        let payload = Payload {
            to: self
                .cfg
                .to
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect(),
            text: format!("{subject}\n{body}"),
            encoding: "standard",
        };
        let resp = self
            .client
            .post(&url)
            .header("access-token", &self.cfg.access_token)
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
