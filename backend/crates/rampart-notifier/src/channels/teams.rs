//! Microsoft Teams via incoming webhook (MessageCard format).
//!
//! Setup: in a Teams channel, "Connectors" → "Incoming Webhook" → copy URL.
//! The MessageCard format is older than adaptive cards but works with all
//! Teams webhook endpoints, including the deprecated "Office 365 Connectors"
//! ones still in use widely.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct TeamsConfig {
    pub webhook_url: String,
}

#[derive(Debug)]
pub struct Teams {
    cfg:    TeamsConfig,
    client: reqwest::Client,
}

impl Teams {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: TeamsConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if !cfg.webhook_url.contains("webhook.office.com")
            && !cfg.webhook_url.contains("office365.com")
            && !cfg.webhook_url.contains("microsoft.com") {
            // Don't hard-fail; users sometimes proxy these. Just warn-shape via BadConfig
            // if the URL is obviously wrong.
            if !cfg.webhook_url.starts_with("https://") {
                return Err(ChannelError::BadConfig("webhook_url must be https".into()));
            }
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[derive(Serialize)]
struct MessageCard<'a> {
    #[serde(rename = "@type")]    type_:        &'a str,
    #[serde(rename = "@context")] context:      &'a str,
    summary:    &'a str,
    #[serde(rename = "themeColor")] theme_color: String,
    title:      &'a str,
    text:       &'a str,
}

#[async_trait]
impl Channel for Teams {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError> {
        let theme_color = match event.heartbeat.status {
            rampart_core::MonitorStatus::Up   => "00B894",
            rampart_core::MonitorStatus::Down => "EF4444",
            _                                 => "F59E0B",
        }.to_string();

        let card = MessageCard {
            type_:       "MessageCard",
            context:     "https://schema.org/extensions",
            summary:     subject,
            theme_color,
            title:       subject,
            text:        body,
        };
        let resp = self.client.post(&self.cfg.webhook_url).json(&card).send().await?;
        if !resp.status().is_success() {
            let code = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(ChannelError::Upstream(code, body));
        }
        Ok(())
    }
}
