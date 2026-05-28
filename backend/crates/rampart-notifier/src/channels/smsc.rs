//! SMSC.ru — GET https://smsc.ru/sys/send.php?login=...&psw=...&phones=...&mes=...

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct SmscConfig {
    pub login: String,
    pub psw: String,
    /// comma-separated phones
    pub phones: String,
}

pub struct Smsc {
    cfg: SmscConfig,
    client: reqwest::Client,
}

impl Smsc {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: SmscConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.login.is_empty() || cfg.psw.is_empty() || cfg.phones.is_empty() {
            return Err(ChannelError::BadConfig(
                "login + psw + phones required".into(),
            ));
        }
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }
}

#[async_trait]
impl Channel for Smsc {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let text = format!("{subject}\n{body}");
        let resp = self
            .client
            .get("https://smsc.ru/sys/send.php")
            .query(&[
                ("login", self.cfg.login.as_str()),
                ("psw", self.cfg.psw.as_str()),
                ("phones", self.cfg.phones.as_str()),
                ("mes", text.as_str()),
                ("charset", "utf-8"),
                ("fmt", "3"), // JSON response
            ])
            .send()
            .await?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(ChannelError::Upstream(status.as_u16(), body));
        }
        // SMSC returns {"error":...} on failure.
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
            if v.get("error_code").is_some() || v.get("error").is_some() {
                return Err(ChannelError::Upstream(200, body));
            }
        }
        Ok(())
    }
}
