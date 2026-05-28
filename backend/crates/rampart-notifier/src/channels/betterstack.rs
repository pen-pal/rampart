//! BetterStack (formerly Better Uptime) — POST integration_url with
//! {summary, description}.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use rampart_core::MonitorStatus;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct BetterstackConfig {
    pub integration_url: String,
}

pub struct Betterstack {
    cfg: BetterstackConfig,
    client: reqwest::Client,
}

impl Betterstack {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: BetterstackConfig = serde_json::from_value(raw.clone())
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
    summary: &'a str,
    description: &'a str,
    severity: &'a str,
    requester_email: &'static str,
}

#[async_trait]
impl Channel for Betterstack {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError> {
        let severity = if event.heartbeat.status == MonitorStatus::Up {
            "info"
        } else {
            "critical"
        };
        let payload = Payload {
            summary: subject,
            description: body,
            severity,
            requester_email: "rampart@local",
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
