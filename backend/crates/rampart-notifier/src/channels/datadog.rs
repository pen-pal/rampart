//! Datadog Events — POST /api/v1/events with DD-API-KEY header.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use rampart_core::MonitorStatus;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct DatadogConfig {
    pub api_key: String,
    /// "us1" | "us3" | "us5" | "eu" | "us1-fed"
    #[serde(default = "default_site")]
    pub site: String,
}
fn default_site() -> String {
    "us1".into()
}

pub struct Datadog {
    cfg: DatadogConfig,
    client: reqwest::Client,
}

impl Datadog {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: DatadogConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.api_key.is_empty() {
            return Err(ChannelError::BadConfig("api_key required".into()));
        }
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    title: &'a str,
    text: &'a str,
    alert_type: &'a str,
    source_type_name: &'static str,
    tags: Vec<String>,
}

#[async_trait]
impl Channel for Datadog {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError> {
        let alert_type = match event.heartbeat.status {
            MonitorStatus::Up => "success",
            MonitorStatus::Warn => "warning",
            _ => "error",
        };
        let host = match self.cfg.site.as_str() {
            "eu" => "https://api.datadoghq.eu",
            "us3" => "https://api.us3.datadoghq.com",
            "us5" => "https://api.us5.datadoghq.com",
            "us1-fed" => "https://api.ddog-gov.com",
            _ => "https://api.datadoghq.com",
        };
        let payload = Payload {
            title: subject,
            text: body,
            alert_type,
            source_type_name: "rampart",
            tags: vec![
                format!("monitor:{}", event.monitor.name),
                format!("monitor_id:{}", event.monitor.id.0),
            ],
        };
        let resp = self
            .client
            .post(format!("{host}/api/v1/events"))
            .header("DD-API-KEY", &self.cfg.api_key)
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
