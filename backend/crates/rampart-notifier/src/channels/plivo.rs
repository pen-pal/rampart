//! Plivo SMS — POST /v1/Account/<auth_id>/Message/ with basic auth.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct PlivoConfig {
    pub auth_id: String,
    pub auth_token: String,
    pub from: String,
    /// "+15551234567<+15559876543>" — Plivo accepts <>-separated multi.
    pub to: String,
}

pub struct Plivo {
    cfg: PlivoConfig,
    client: reqwest::Client,
}

impl Plivo {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: PlivoConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.auth_id.is_empty() || cfg.auth_token.is_empty() || cfg.to.is_empty() {
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
    src: &'a str,
    dst: &'a str,
    text: String,
}

#[async_trait]
impl Channel for Plivo {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let url = format!(
            "https://api.plivo.com/v1/Account/{}/Message/",
            self.cfg.auth_id
        );
        let payload = Payload {
            src: &self.cfg.from,
            dst: &self.cfg.to,
            text: format!("{subject}\n{body}"),
        };
        let resp = self
            .client
            .post(&url)
            .basic_auth(&self.cfg.auth_id, Some(&self.cfg.auth_token))
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
