//! New Relic Events — POST /v1/accounts/<account_id>/events with Api-Key.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct NewrelicConfig {
    pub insert_key: String,
    pub account_id: String,
    /// "us" or "eu"
    #[serde(default = "default_region")]
    pub region: String,
}
fn default_region() -> String {
    "us".into()
}

pub struct Newrelic {
    cfg: NewrelicConfig,
    client: reqwest::Client,
}

impl Newrelic {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: NewrelicConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.insert_key.is_empty() || cfg.account_id.is_empty() {
            return Err(ChannelError::BadConfig(
                "insert_key + account_id required".into(),
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
    #[serde(rename = "eventType")]
    event_type: &'static str,
    title: &'a str,
    body: &'a str,
    monitor: &'a str,
    monitor_id: String,
    status: &'a str,
}

#[async_trait]
impl Channel for Newrelic {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError> {
        let host = if self.cfg.region == "eu" {
            "https://insights-collector.eu01.nr-data.net"
        } else {
            "https://insights-collector.newrelic.com"
        };
        let url = format!("{host}/v1/accounts/{}/events", self.cfg.account_id);
        let payload = vec![Payload {
            event_type: "RampartAlert",
            title: subject,
            body,
            monitor: &event.monitor.name,
            monitor_id: event.monitor.id.0.to_string(),
            status: event.status_str(),
        }];
        let resp = self
            .client
            .post(&url)
            .header("Api-Key", &self.cfg.insert_key)
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
