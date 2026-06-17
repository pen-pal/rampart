//! Gotify — self-hosted push server.
//!
//! Setup: deploy https://gotify.net, create an Application in the Gotify
//! UI, copy the Application Token. Point this channel at the server URL +
//! token. Subscribers use the Gotify mobile/desktop client.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct GotifyConfig {
    pub server: String,
    pub token: String,
    /// Gotify priority 0..10; 5 is the default visual threshold.
    #[serde(default = "default_priority")]
    pub priority: i32,
}
fn default_priority() -> i32 {
    5
}

#[derive(Debug)]
pub struct Gotify {
    cfg: GotifyConfig,
    client: reqwest::Client,
}

impl Gotify {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: GotifyConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.server.is_empty() || cfg.token.is_empty() {
            return Err(ChannelError::BadConfig(
                "server and token are required".into(),
            ));
        }
        Ok(Self {
            cfg,
            client: crate::http::client(),
        })
    }
}

#[derive(Serialize)]
struct GotifyPayload<'a> {
    title: &'a str,
    message: &'a str,
    priority: i32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn requires_server_and_token() {
        assert!(Gotify::from_config(&json!({"server": "https://gotify.example.com"})).is_err());
        assert!(Gotify::from_config(&json!({"token":  "x"})).is_err());
    }

    #[test]
    fn defaults_priority_to_5() {
        let raw = json!({"server": "https://gotify.example.com", "token": "x"});
        let cfg: GotifyConfig = serde_json::from_value(raw).unwrap();
        assert_eq!(cfg.priority, 5);
    }
}

#[async_trait]
impl Channel for Gotify {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let url = format!(
            "{}/message?token={}",
            self.cfg.server.trim_end_matches('/'),
            self.cfg.token
        );
        let payload = GotifyPayload {
            title: subject,
            message: body,
            priority: self.cfg.priority,
        };
        let resp = self.client.post(&url).json(&payload).send().await?;
        if !resp.status().is_success() {
            let code = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(ChannelError::Upstream(code, body));
        }
        Ok(())
    }
}
