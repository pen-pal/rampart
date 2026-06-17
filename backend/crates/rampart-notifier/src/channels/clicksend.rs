//! ClickSend SMS — REST /v3/sms/send with basic auth.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct ClicksendConfig {
    pub username: String,
    pub api_key: String,
    pub from: String,
    /// comma-separated E.164 numbers
    pub to: String,
}

pub struct Clicksend {
    cfg: ClicksendConfig,
    client: reqwest::Client,
}

impl Clicksend {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: ClicksendConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.api_key.trim().is_empty() || cfg.to.trim().is_empty() {
            return Err(ChannelError::BadConfig("api_key and to required".into()));
        }
        Ok(Self {
            cfg,
            client: crate::http::client(),
        })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    messages: Vec<Sms<'a>>,
}
#[derive(Serialize)]
struct Sms<'a> {
    source: &'static str,
    from: &'a str,
    to: &'a str,
    body: String,
}

#[async_trait]
impl Channel for Clicksend {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let text = format!("{subject}\n{body}");
        let payload = Payload {
            messages: self
                .cfg
                .to
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|num| Sms {
                    source: "rampart",
                    from: &self.cfg.from,
                    to: num,
                    body: text.clone(),
                })
                .collect(),
        };
        let resp = self
            .client
            .post("https://rest.clicksend.com/v3/sms/send")
            .basic_auth(&self.cfg.username, Some(&self.cfg.api_key))
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
