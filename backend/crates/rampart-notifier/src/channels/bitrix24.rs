//! Bitrix24 — REST API im.notify.system.add to push to a user.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Bitrix24Config {
    pub webhook_url: String,
    pub user_id: String,
}

pub struct Bitrix24 {
    cfg: Bitrix24Config,
    client: reqwest::Client,
}

impl Bitrix24 {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: Bitrix24Config = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if !cfg.webhook_url.starts_with("http") || cfg.user_id.trim().is_empty() {
            return Err(ChannelError::BadConfig(
                "webhook_url and user_id required".into(),
            ));
        }
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }
}

#[async_trait]
impl Channel for Bitrix24 {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let url = format!(
            "{}/im.notify.system.add.json",
            self.cfg.webhook_url.trim_end_matches('/')
        );
        let message = format!("{subject}\n{body}");
        let resp = self
            .client
            .post(&url)
            .form(&[
                ("USER_ID", self.cfg.user_id.as_str()),
                ("MESSAGE", &message),
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
