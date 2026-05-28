//! Splash — incident management via incoming webhook.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use rampart_core::MonitorStatus;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct SplashConfig {
    pub webhook_url: String,
}

pub struct Splash {
    cfg: SplashConfig,
    client: reqwest::Client,
}

impl Splash {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: SplashConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if !cfg.webhook_url.starts_with("http") {
            return Err(ChannelError::BadConfig("webhook_url required".into()));
        }
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    summary: &'a str,
    details: &'a str,
    status: &'a str,
    key: String,
}

#[async_trait]
impl Channel for Splash {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError> {
        let status = if event.heartbeat.status == MonitorStatus::Up {
            "resolved"
        } else {
            "triggered"
        };
        let payload = Payload {
            summary: subject,
            details: body,
            status,
            key: format!("rampart-monitor-{}", event.monitor.id.0),
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
