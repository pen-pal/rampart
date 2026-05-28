//! SerwerSMS.pl — POST /api/v2/messages/send_sms.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct SerwerSmsConfig {
    pub username: String,
    pub password: String,
    pub sender: String,
    /// comma-separated phone numbers
    pub phone: String,
}

pub struct Serwersms {
    cfg: SerwerSmsConfig,
    client: reqwest::Client,
}

impl Serwersms {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: SerwerSmsConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.username.is_empty() || cfg.password.is_empty() || cfg.phone.is_empty() {
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
    username: &'a str,
    password: &'a str,
    phone: &'a str,
    text: String,
    sender: &'a str,
}

#[async_trait]
impl Channel for Serwersms {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let payload = Payload {
            username: &self.cfg.username,
            password: &self.cfg.password,
            phone: &self.cfg.phone,
            text: format!("{subject}\n{body}"),
            sender: &self.cfg.sender,
        };
        let resp = self
            .client
            .post("https://api2.serwersms.pl/messages/send_sms")
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
