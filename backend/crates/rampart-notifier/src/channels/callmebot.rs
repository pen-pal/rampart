//! CallMeBot — free WhatsApp / Signal / Telegram push via api.callmebot.com.
//! Endpoint pattern varies by service; we GET the user-supplied URL with
//! the message appended as `?text=...`.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct CallmebotConfig {
    pub endpoint_url: String,
}

pub struct Callmebot {
    cfg: CallmebotConfig,
    client: reqwest::Client,
}

impl Callmebot {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: CallmebotConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if !cfg.endpoint_url.starts_with("http") {
            return Err(ChannelError::BadConfig("endpoint_url required".into()));
        }
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }
}

#[async_trait]
impl Channel for Callmebot {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let text = format!("{subject}\n{body}");
        let resp = self
            .client
            .get(&self.cfg.endpoint_url)
            .query(&[("text", text)])
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
