//! Pushover — pushover.net.
//!
//! Setup: create an Application on pushover.net to get an API token,
//! note your User Key from the dashboard. Optional `device` targets a
//! specific device; otherwise delivers to all devices on the user account.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct PushoverConfig {
    pub api_token: String,
    pub user_key:  String,
    /// Priority -2..2; 1 = high (bypass quiet hours), 2 = emergency (acks required).
    #[serde(default)]
    pub priority:  Option<i32>,
    /// Optional sound name (default "pushover"). See pushover.net/api#sounds.
    #[serde(default)]
    pub sound:     Option<String>,
    #[serde(default)]
    pub device:    Option<String>,
}

#[derive(Debug)]
pub struct Pushover {
    cfg:    PushoverConfig,
    client: reqwest::Client,
}

impl Pushover {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: PushoverConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.api_token.is_empty() || cfg.user_key.is_empty() {
            return Err(ChannelError::BadConfig("api_token and user_key are required".into()));
        }
        if let Some(p) = cfg.priority {
            if !(-2..=2).contains(&p) {
                return Err(ChannelError::BadConfig("priority must be between -2 and 2".into()));
            }
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn requires_both_tokens() {
        assert!(Pushover::from_config(&json!({"api_token": "x"})).is_err());
        assert!(Pushover::from_config(&json!({"user_key":  "x"})).is_err());
    }

    #[test]
    fn rejects_priority_out_of_range() {
        let err = Pushover::from_config(&json!({"api_token": "x", "user_key": "y", "priority": 7})).unwrap_err();
        assert!(matches!(err, ChannelError::BadConfig(_)));
    }

    #[test]
    fn accepts_valid_priority_range() {
        for p in -2..=2 {
            assert!(Pushover::from_config(&json!({"api_token": "x", "user_key": "y", "priority": p})).is_ok(),
                "priority {p} should be valid");
        }
    }
}

#[async_trait]
impl Channel for Pushover {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let mut form = HashMap::new();
        form.insert("token",   self.cfg.api_token.as_str());
        form.insert("user",    self.cfg.user_key.as_str());
        form.insert("title",   subject);
        form.insert("message", body);
        let pr;
        if let Some(p) = self.cfg.priority {
            pr = p.to_string();
            form.insert("priority", &pr);
        }
        if let Some(s) = &self.cfg.sound  { form.insert("sound",  s); }
        if let Some(d) = &self.cfg.device { form.insert("device", d); }
        // Emergency priority requires retry/expire — give safe defaults.
        if self.cfg.priority == Some(2) {
            form.insert("retry",  "60");
            form.insert("expire", "3600");
        }

        let resp = self.client
            .post("https://api.pushover.net/1/messages.json")
            .form(&form)
            .send().await?;
        if !resp.status().is_success() {
            let code = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(ChannelError::Upstream(code, body));
        }
        Ok(())
    }
}
