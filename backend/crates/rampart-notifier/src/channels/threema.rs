//! Threema Gateway — simple text mode.
//! GET https://msgapi.threema.ch/send_simple?from=...&to=...&secret=...&text=...

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ThreemaConfig {
    pub gateway_id: String,
    pub secret: String,
    /// Recipient — either an 8-char Threema ID, an email, or phone.
    pub to: String,
}

pub struct Threema {
    cfg: ThreemaConfig,
    client: reqwest::Client,
}

impl Threema {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: ThreemaConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.gateway_id.trim().is_empty() || cfg.secret.is_empty() || cfg.to.is_empty() {
            return Err(ChannelError::BadConfig(
                "gateway_id + secret + to required".into(),
            ));
        }
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }
}

#[async_trait]
impl Channel for Threema {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let text = format!("{subject}\n{body}");
        // Threema's simple mode expects either `to`, `email`, or `phone`.
        let to_key = if self.cfg.to.contains('@') {
            "email"
        } else if self.cfg.to.starts_with('+') {
            "phone"
        } else {
            "to"
        };
        let resp = self
            .client
            .post("https://msgapi.threema.ch/send_simple")
            .form(&[
                ("from", self.cfg.gateway_id.as_str()),
                ("secret", self.cfg.secret.as_str()),
                (to_key, self.cfg.to.as_str()),
                ("text", text.as_str()),
            ])
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
