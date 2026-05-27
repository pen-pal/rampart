//! Bandwidth SMS — POST /api/v2/users/<account>/messages with basic auth.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct BandwidthConfig {
    pub account_id:     String,
    pub username:       String,
    pub password:       String,
    pub application_id: String,
    pub from:           String,
    /// comma-separated E.164 numbers
    pub to:             String,
}

pub struct Bandwidth { cfg: BandwidthConfig, client: reqwest::Client }

impl Bandwidth {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: BandwidthConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.account_id.is_empty() || cfg.username.is_empty() || cfg.password.is_empty() || cfg.to.is_empty() {
            return Err(ChannelError::BadConfig("missing required fields".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    #[serde(rename = "applicationId")]
    application_id: &'a str,
    to:             Vec<&'a str>,
    from:           &'a str,
    text:           String,
}

#[async_trait]
impl Channel for Bandwidth {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let url = format!(
            "https://messaging.bandwidth.com/api/v2/users/{}/messages",
            self.cfg.account_id,
        );
        let payload = Payload {
            application_id: &self.cfg.application_id,
            to: self.cfg.to.split(',').map(str::trim).filter(|s| !s.is_empty()).collect(),
            from: &self.cfg.from,
            text: format!("{subject}\n{body}"),
        };
        let resp = self.client.post(&url)
            .basic_auth(&self.cfg.username, Some(&self.cfg.password))
            .json(&payload).send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }
}
