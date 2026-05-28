//! Grafana OnCall — webhook integration URL with state + title + message.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use rampart_core::MonitorStatus;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct GrafanaOncallConfig {
    pub webhook_url: String,
}

pub struct GrafanaOncall {
    cfg: GrafanaOncallConfig,
    client: reqwest::Client,
}

impl GrafanaOncall {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: GrafanaOncallConfig = serde_json::from_value(raw.clone())
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
    alert_uid: String,
    title: &'a str,
    message: &'a str,
    state: &'a str,
    link_to_upstream_details: String,
}

#[async_trait]
impl Channel for GrafanaOncall {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError> {
        let state = if event.heartbeat.status == MonitorStatus::Up {
            "resolved"
        } else {
            "alerting"
        };
        let payload = Payload {
            alert_uid: format!("rampart-monitor-{}", event.monitor.id.0),
            title: subject,
            message: body,
            state,
            link_to_upstream_details: format!("rampart://monitor/{}", event.monitor.id.0),
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
