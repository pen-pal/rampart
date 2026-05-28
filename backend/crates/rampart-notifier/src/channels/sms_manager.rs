//! SmsManager.cz — GET https://http-api.smsmanager.cz/Send?apikey=...&number=...&message=...

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct SmsManagerConfig {
    pub api_key: String,
    /// comma-separated CZ numbers
    pub numbers: String,
    /// e.g. "lowcost", "economy", "high".
    #[serde(default = "default_quality")]
    pub quality: String,
    #[serde(default)]
    pub sender_id: Option<String>,
}
fn default_quality() -> String {
    "economy".into()
}

pub struct SmsManager {
    cfg: SmsManagerConfig,
    client: reqwest::Client,
}

impl SmsManager {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: SmsManagerConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.api_key.is_empty() || cfg.numbers.is_empty() {
            return Err(ChannelError::BadConfig("api_key + numbers required".into()));
        }
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }
}

#[async_trait]
impl Channel for SmsManager {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let text = format!("{subject}\n{body}");
        let mut params = vec![
            ("apikey", self.cfg.api_key.clone()),
            ("number", self.cfg.numbers.clone()),
            ("message", text),
            ("gateway", self.cfg.quality.clone()),
        ];
        if let Some(s) = &self.cfg.sender_id {
            params.push(("senderid", s.clone()));
        }
        let resp = self
            .client
            .get("https://http-api.smsmanager.cz/Send")
            .query(&params)
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
