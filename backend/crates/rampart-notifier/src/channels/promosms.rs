//! PromoSMS.pl — POST https://api.promosms.com/api/rest/v3_2/sms with basic auth.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct PromosmsConfig {
    pub username: String,
    pub password: String,
    pub sender: String,
    pub to: String,
    /// "1" (ECO), "3" (Full)
    #[serde(default = "default_type")]
    pub kind: String,
}
fn default_type() -> String {
    "3".into()
}

pub struct Promosms {
    cfg: PromosmsConfig,
    client: reqwest::Client,
}

impl Promosms {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: PromosmsConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.username.is_empty() || cfg.password.is_empty() || cfg.to.is_empty() {
            return Err(ChannelError::BadConfig("missing required fields".into()));
        }
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    text: String,
    sender: &'a str,
    recipients: Vec<&'a str>,
    #[serde(rename = "type")]
    kind: &'a str,
}

#[async_trait]
impl Channel for Promosms {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let payload = Payload {
            text: format!("{subject}\n{body}"),
            sender: &self.cfg.sender,
            recipients: self
                .cfg
                .to
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect(),
            kind: &self.cfg.kind,
        };
        let resp = self
            .client
            .post("https://api.promosms.com/api/rest/v3_2/sms")
            .basic_auth(&self.cfg.username, Some(&self.cfg.password))
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
