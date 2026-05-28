//! Opsgenie — alert API. Creates an alert; resolves on Up.
//!
//! `priority` defaults to P3. We use the monitor.id as `alias` so the
//! same monitor's repeated outages collapse server-side and the resolve
//! call hits the right alert.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use rampart_core::MonitorStatus;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct OpsgenieConfig {
    pub api_key: String,
    /// EU API endpoint is at api.eu.opsgenie.com; default US.
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default = "default_priority")]
    pub priority: String,
}
fn default_priority() -> String {
    "P3".into()
}

pub struct Opsgenie {
    cfg: OpsgenieConfig,
    client: reqwest::Client,
}

impl Opsgenie {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: OpsgenieConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.api_key.trim().is_empty() {
            return Err(ChannelError::BadConfig("api_key required".into()));
        }
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }
    fn base(&self) -> &'static str {
        match self.cfg.region.as_deref() {
            Some("eu") => "https://api.eu.opsgenie.com",
            _ => "https://api.opsgenie.com",
        }
    }
}

#[derive(Serialize)]
struct CreateAlert<'a> {
    message: &'a str,
    alias: String,
    description: &'a str,
    priority: &'a str,
    source: &'static str,
}

#[async_trait]
impl Channel for Opsgenie {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError> {
        let alias = format!("rampart-monitor-{}", event.monitor.id.0);
        if event.heartbeat.status == MonitorStatus::Up {
            // Close on recovery — server-side dedupe by alias.
            let url = format!(
                "{}/v2/alerts/{}/close?identifierType=alias",
                self.base(),
                alias
            );
            let resp = self
                .client
                .post(&url)
                .header("Authorization", format!("GenieKey {}", self.cfg.api_key))
                .json(&serde_json::json!({ "source": "rampart", "note": body }))
                .send()
                .await?;
            // Already-closed is fine — return Ok.
            if !resp.status().is_success() && resp.status().as_u16() != 404 {
                return Err(ChannelError::Upstream(
                    resp.status().as_u16(),
                    resp.text().await.unwrap_or_default(),
                ));
            }
            return Ok(());
        }
        let payload = CreateAlert {
            message: subject,
            alias,
            description: body,
            priority: &self.cfg.priority,
            source: "rampart",
        };
        let resp = self
            .client
            .post(format!("{}/v2/alerts", self.base()))
            .header("Authorization", format!("GenieKey {}", self.cfg.api_key))
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
