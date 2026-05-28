//! Cellsynt — GET https://se-1.cellsynt.net/sms.php with username + password.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct CellsyntConfig {
    pub username: String,
    pub password: String,
    pub originator: String,
    /// comma-separated E.164 numbers — e.g. "0046701234567"
    pub destination: String,
}

pub struct Cellsynt {
    cfg: CellsyntConfig,
    client: reqwest::Client,
}

impl Cellsynt {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: CellsyntConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.username.is_empty() || cfg.password.is_empty() || cfg.destination.is_empty() {
            return Err(ChannelError::BadConfig(
                "username + password + destination required".into(),
            ));
        }
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }
}

#[async_trait]
impl Channel for Cellsynt {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let text = format!("{subject}\n{body}");
        let resp = self
            .client
            .get("https://se-1.cellsynt.net/sms.php")
            .query(&[
                ("username", self.cfg.username.as_str()),
                ("password", self.cfg.password.as_str()),
                ("originatortype", "alpha"),
                ("originator", self.cfg.originator.as_str()),
                ("destination", self.cfg.destination.as_str()),
                ("text", text.as_str()),
                ("charset", "UTF-8"),
            ])
            .send()
            .await?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(ChannelError::Upstream(status.as_u16(), body));
        }
        // Cellsynt returns "OK: <id>" on success or "Error: ..." on failure.
        if body.starts_with("Error") {
            return Err(ChannelError::Upstream(200, body));
        }
        Ok(())
    }
}
