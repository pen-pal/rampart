//! FlashDuty (快猫星云) — POST integration_url with event payload.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use rampart_core::MonitorStatus;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct FlashdutyConfig {
    pub integration_url: String,
    #[serde(default = "default_severity")]
    pub severity: String,
}
fn default_severity() -> String {
    "Warning".into()
}

pub struct Flashduty {
    cfg: FlashdutyConfig,
    client: reqwest::Client,
}

impl Flashduty {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: FlashdutyConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if !cfg.integration_url.starts_with("http") {
            return Err(ChannelError::BadConfig("integration_url required".into()));
        }
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    event_status: &'a str,
    severity: &'a str,
    title_rule: String,
    description: &'a str,
    alert_key: String,
}

#[async_trait]
impl Channel for Flashduty {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError> {
        let status = if event.heartbeat.status == MonitorStatus::Up {
            "Ok"
        } else {
            "Critical"
        };
        let payload = Payload {
            event_status: status,
            severity: &self.cfg.severity,
            title_rule: subject.into(),
            description: body,
            alert_key: format!("rampart-monitor-{}", event.monitor.id.0),
        };
        let resp = self
            .client
            .post(&self.cfg.integration_url)
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
