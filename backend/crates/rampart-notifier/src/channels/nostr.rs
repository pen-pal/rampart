//! Nostr DM via a user-supplied relay HTTP bridge.
//!
//! v1 implementation: POST the user's bridge endpoint with {recipient,
//! content}. Native nostr signing + WebSocket relay would pull in
//! schnorr + tokio-tungstenite for one channel; far better to defer
//! that to an external nostr-bridge container the user already runs.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct NostrConfig {
    pub bridge_url: String,
    pub recipient:  String,
    #[serde(default)]
    pub api_key:    Option<String>,
}

pub struct Nostr { cfg: NostrConfig, client: reqwest::Client }

impl Nostr {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: NostrConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if !cfg.bridge_url.starts_with("http") || cfg.recipient.is_empty() {
            return Err(ChannelError::BadConfig("bridge_url + recipient required".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    recipient: &'a str,
    content:   String,
}

#[async_trait]
impl Channel for Nostr {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let payload = Payload { recipient: &self.cfg.recipient, content: format!("{subject}\n{body}") };
        let mut req = self.client.post(&self.cfg.bridge_url).json(&payload);
        if let Some(k) = &self.cfg.api_key { req = req.bearer_auth(k); }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }
}
