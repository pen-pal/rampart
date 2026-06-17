//! Heii On-Call — POST trigger URL with optional close hook on recovery.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use rampart_core::MonitorStatus;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct HeiiOncallConfig {
    pub trigger_url: String,
    #[serde(default)]
    pub close_url: Option<String>,
}

pub struct HeiiOncall {
    cfg: HeiiOncallConfig,
    client: reqwest::Client,
}

impl HeiiOncall {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: HeiiOncallConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if !cfg.trigger_url.starts_with("http") {
            return Err(ChannelError::BadConfig("trigger_url required".into()));
        }
        Ok(Self {
            cfg,
            client: crate::http::client(),
        })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    subject: &'a str,
    body: &'a str,
    external_id: String,
}

#[async_trait]
impl Channel for HeiiOncall {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError> {
        let url = if event.heartbeat.status == MonitorStatus::Up {
            self.cfg
                .close_url
                .as_deref()
                .unwrap_or(&self.cfg.trigger_url)
        } else {
            &self.cfg.trigger_url
        };
        let payload = Payload {
            subject,
            body,
            external_id: format!("rampart-monitor-{}", event.monitor.id.0),
        };
        let resp = self.client.post(url).json(&payload).send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(
                resp.status().as_u16(),
                resp.text().await.unwrap_or_default(),
            ));
        }
        Ok(())
    }
}
