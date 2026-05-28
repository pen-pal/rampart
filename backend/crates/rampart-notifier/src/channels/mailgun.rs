//! Mailgun — POST https://api.mailgun.net/v3/<domain>/messages.
//! Basic auth: "api" + private API key. EU users override base_url.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct MailgunConfig {
    pub api_key: String,
    pub domain: String,
    #[serde(default = "default_base")]
    pub base_url: String,
    pub from: String,
    /// comma-separated emails
    pub to: String,
}
fn default_base() -> String {
    "https://api.mailgun.net".into()
}

pub struct Mailgun {
    cfg: MailgunConfig,
    client: reqwest::Client,
}

impl Mailgun {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: MailgunConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.api_key.is_empty() || cfg.domain.is_empty() || cfg.to.is_empty() {
            return Err(ChannelError::BadConfig("missing required fields".into()));
        }
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }
}

#[async_trait]
impl Channel for Mailgun {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let url = format!(
            "{}/v3/{}/messages",
            self.cfg.base_url.trim_end_matches('/'),
            self.cfg.domain
        );
        let resp = self
            .client
            .post(&url)
            .basic_auth("api", Some(&self.cfg.api_key))
            .form(&[
                ("from", self.cfg.from.as_str()),
                ("to", self.cfg.to.as_str()),
                ("subject", subject),
                ("text", body),
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
