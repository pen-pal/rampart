//! Twilio SMS.
//!
//! Setup: sign up at twilio.com, buy a phone number, copy AccountSID
//! and AuthToken from the console. Both `from` and `to` numbers must be
//! E.164 format ("+15551234567"). Multiple recipients are split per SMS
//! because Twilio doesn't fan out a single API call to multiple numbers.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct TwilioConfig {
    pub account_sid: String,
    pub auth_token: String,
    pub from: String,
    /// Comma-separated recipients in E.164 format.
    pub to: String,
}

#[derive(Debug)]
pub struct Twilio {
    cfg: TwilioConfig,
    client: reqwest::Client,
}

impl Twilio {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: TwilioConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.account_sid.is_empty() || cfg.auth_token.is_empty() {
            return Err(ChannelError::BadConfig(
                "account_sid and auth_token are required".into(),
            ));
        }
        if !cfg.from.starts_with('+') {
            return Err(ChannelError::BadConfig(
                "from number must be in E.164 format (e.g. +15551234567)".into(),
            ));
        }
        if cfg.to.is_empty() {
            return Err(ChannelError::BadConfig(
                "at least one to number is required".into(),
            ));
        }
        Ok(Self {
            cfg,
            client: crate::http::client(),
        })
    }
}

#[async_trait]
impl Channel for Twilio {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        // SMS body: subject + truncated body, hard-capped at 1600 chars
        // (Twilio's segmented SMS limit). One API call per recipient.
        // Truncate by CHARS, not bytes — a byte slice through a multi-byte
        // char (emoji/CJK in a monitor name or error body) panics, and the
        // panic in the spawned dispatch task silently drops the page.
        let combined = format!("{subject}\n\n{body}");
        let text: String = combined.chars().take(1600).collect();

        let url = format!(
            "https://api.twilio.com/2010-04-01/Accounts/{}/Messages.json",
            self.cfg.account_sid
        );

        for raw in self.cfg.to.split(',') {
            let to = raw.trim();
            if to.is_empty() {
                continue;
            }
            if !to.starts_with('+') {
                return Err(ChannelError::BadConfig(format!(
                    "to number {to:?} not in E.164 format"
                )));
            }
            let mut form = HashMap::new();
            form.insert("From", self.cfg.from.as_str());
            form.insert("To", to);
            form.insert("Body", text.as_str());

            let resp = self
                .client
                .post(&url)
                .basic_auth(&self.cfg.account_sid, Some(&self.cfg.auth_token))
                .form(&form)
                .send()
                .await?;
            if !resp.status().is_success() {
                let code = resp.status().as_u16();
                let body = resp.text().await.unwrap_or_default();
                return Err(ChannelError::Upstream(code, body));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn base() -> serde_json::Value {
        json!({
            "account_sid": "AC1", "auth_token": "tok",
            "from": "+15551112222", "to": "+15553334444"
        })
    }

    #[test]
    fn requires_all_credentials() {
        for k in ["account_sid", "auth_token"] {
            let mut v = base();
            v.as_object_mut().unwrap().remove(k);
            assert!(Twilio::from_config(&v).is_err(), "missing {k} should fail");
        }
    }

    #[test]
    fn rejects_from_without_e164() {
        let mut v = base();
        v["from"] = json!("5551112222");
        assert!(Twilio::from_config(&v).is_err());
    }

    #[test]
    fn rejects_empty_to() {
        let mut v = base();
        v["to"] = json!("");
        assert!(Twilio::from_config(&v).is_err());
    }

    #[test]
    fn accepts_valid_config() {
        crate::init_test_crypto();
        assert!(Twilio::from_config(&base()).is_ok());
    }
}
