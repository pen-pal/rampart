//! seven.io (formerly sms77) — POST https://gateway.seven.io/api/sms.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct SevenioConfig {
    pub api_key: String,
    /// comma-separated E.164 numbers
    pub to: String,
    #[serde(default)]
    pub from: Option<String>,
}

pub struct Sevenio {
    cfg: SevenioConfig,
    client: reqwest::Client,
}

impl Sevenio {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: SevenioConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.api_key.is_empty() || cfg.to.is_empty() {
            return Err(ChannelError::BadConfig("api_key + to required".into()));
        }
        Ok(Self {
            cfg,
            client: crate::http::client(),
        })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    to: &'a str,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    from: &'a Option<String>,
}

#[async_trait]
impl Channel for Sevenio {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let payload = Payload {
            to: &self.cfg.to,
            text: format!("{subject}\n{body}"),
            from: &self.cfg.from,
        };
        let resp = self
            .client
            .post("https://gateway.seven.io/api/sms")
            .header("X-Api-Key", &self.cfg.api_key)
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
