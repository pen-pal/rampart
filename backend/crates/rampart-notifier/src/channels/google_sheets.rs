//! Google Sheets — append a row via a deployed Apps Script web-app URL.
//!
//! The user deploys a small Apps Script that takes {timestamp, title,
//! body, status} and appends to a sheet. We POST the JSON. Avoids the
//! OAuth dance entirely.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct GoogleSheetsConfig {
    pub webhook_url: String,
}

pub struct GoogleSheets {
    cfg: GoogleSheetsConfig,
    client: reqwest::Client,
}

impl GoogleSheets {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: GoogleSheetsConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if !cfg.webhook_url.starts_with("https://script.google.com/") {
            return Err(ChannelError::BadConfig(
                "webhook_url must start with https://script.google.com/".into(),
            ));
        }
        Ok(Self {
            cfg,
            client: crate::http::client(),
        })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    timestamp: String,
    title: &'a str,
    body: &'a str,
    status: String,
    monitor: String,
}

#[async_trait]
impl Channel for GoogleSheets {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError> {
        let payload = Payload {
            timestamp: event
                .heartbeat
                .ts
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default(),
            title: subject,
            body,
            status: event.status_str().to_string(),
            monitor: event.monitor.name.clone(),
        };
        let resp = self
            .client
            .post(&self.cfg.webhook_url)
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
