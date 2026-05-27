//! MessageBird SMS — POST https://rest.messagebird.com/messages with
//! Authorization: AccessKey <key>.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct MessagebirdConfig {
    pub access_key: String,
    pub originator: String,
    /// comma-separated E.164 numbers
    pub recipients: String,
}

pub struct Messagebird { cfg: MessagebirdConfig, client: reqwest::Client }

impl Messagebird {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: MessagebirdConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.access_key.is_empty() || cfg.recipients.is_empty() {
            return Err(ChannelError::BadConfig("access_key + recipients required".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    originator: &'a str,
    recipients: Vec<&'a str>,
    body:       String,
}

#[async_trait]
impl Channel for Messagebird {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let payload = Payload {
            originator: &self.cfg.originator,
            recipients: self.cfg.recipients.split(',').map(str::trim).filter(|s| !s.is_empty()).collect(),
            body: format!("{subject}\n{body}"),
        };
        let resp = self.client.post("https://rest.messagebird.com/messages")
            .header("Authorization", format!("AccessKey {}", self.cfg.access_key))
            .json(&payload).send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }
}
