//! Signal — via signal-cli REST API (self-hosted, https://github.com/
//! bbernhard/signal-cli-rest-api). Posts /v2/send with the sender
//! number, recipients, and message text.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct SignalConfig {
    pub api_url: String,
    pub number: String,
    pub recipients: Vec<String>,
}

pub struct Signal {
    cfg: SignalConfig,
    client: reqwest::Client,
}

impl Signal {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: SignalConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.recipients.is_empty() || cfg.number.trim().is_empty() {
            return Err(ChannelError::BadConfig(
                "number + recipients required".into(),
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
    message: String,
    number: &'a str,
    recipients: &'a Vec<String>,
}

#[async_trait]
impl Channel for Signal {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let url = format!("{}/v2/send", self.cfg.api_url.trim_end_matches('/'));
        let payload = Payload {
            message: format!("{subject}\n{body}"),
            number: &self.cfg.number,
            recipients: &self.cfg.recipients,
        };
        let resp = self.client.post(&url).json(&payload).send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(
                resp.status().as_u16(),
                resp.text().await.unwrap_or_default(),
            ));
        }
        Ok(())
    }
}
