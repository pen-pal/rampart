//! SMS.ir — POST https://api.sms.ir/v1/send/bulk with API key header.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct SmsIrConfig {
    pub api_key:   String,
    pub line_number: String,
    /// comma-separated mobile numbers (IR format)
    pub mobiles:   String,
}

pub struct SmsIr { cfg: SmsIrConfig, client: reqwest::Client }

impl SmsIr {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: SmsIrConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.api_key.is_empty() || cfg.mobiles.is_empty() || cfg.line_number.is_empty() {
            return Err(ChannelError::BadConfig("missing required fields".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    #[serde(rename = "lineNumber")]
    line_number: &'a str,
    #[serde(rename = "messageText")]
    message_text: String,
    mobiles:      Vec<&'a str>,
}

#[async_trait]
impl Channel for SmsIr {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let payload = Payload {
            line_number: &self.cfg.line_number,
            message_text: format!("{subject}\n{body}"),
            mobiles: self.cfg.mobiles.split(',').map(str::trim).filter(|s| !s.is_empty()).collect(),
        };
        let resp = self.client.post("https://api.sms.ir/v1/send/bulk")
            .header("X-API-KEY", &self.cfg.api_key)
            .header("ACCEPT", "application/json")
            .json(&payload).send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }
}
