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
    pub auth_token:  String,
    pub from:        String,
    /// Comma-separated recipients in E.164 format.
    pub to:          String,
}

pub struct Twilio {
    cfg:    TwilioConfig,
    client: reqwest::Client,
}

impl Twilio {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: TwilioConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.account_sid.is_empty() || cfg.auth_token.is_empty() {
            return Err(ChannelError::BadConfig("account_sid and auth_token are required".into()));
        }
        if !cfg.from.starts_with('+') {
            return Err(ChannelError::BadConfig("from number must be in E.164 format (e.g. +15551234567)".into()));
        }
        if cfg.to.is_empty() {
            return Err(ChannelError::BadConfig("at least one to number is required".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[async_trait]
impl Channel for Twilio {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        // SMS body: subject + truncated body, hard-capped at 1600 chars
        // (Twilio's segmented SMS limit). One API call per recipient.
        let combined = format!("{subject}\n\n{body}");
        let text = if combined.len() > 1600 { combined[..1600].to_string() } else { combined };

        let url = format!(
            "https://api.twilio.com/2010-04-01/Accounts/{}/Messages.json",
            self.cfg.account_sid
        );

        for raw in self.cfg.to.split(',') {
            let to = raw.trim();
            if to.is_empty() { continue; }
            if !to.starts_with('+') {
                return Err(ChannelError::BadConfig(format!(
                    "to number {to:?} not in E.164 format"
                )));
            }
            let mut form = HashMap::new();
            form.insert("From", self.cfg.from.as_str());
            form.insert("To",   to);
            form.insert("Body", text.as_str());

            let resp = self.client.post(&url)
                .basic_auth(&self.cfg.account_sid, Some(&self.cfg.auth_token))
                .form(&form)
                .send().await?;
            if !resp.status().is_success() {
                let code = resp.status().as_u16();
                let body = resp.text().await.unwrap_or_default();
                return Err(ChannelError::Upstream(code, body));
            }
        }
        Ok(())
    }
}
